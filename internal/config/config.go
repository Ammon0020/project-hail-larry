// Package config manages persistent configuration for the Local Agent Interface.
// Config is stored in ~/.local-agent/config.json.
// Blueprint references: Sec 20 (Configuration).
package config

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"

	"github.com/adama/local-agent/internal/acp"
)

const (
	appDataDirPerm = 0700
	configFilePerm = 0600
)

// Config is the persistent application configuration.
type Config struct {
	// mu guards mutable fields (Workspaces, Agents) and serializes Save so
	// concurrent HTTP handlers cannot race on slice mutation or interleave
	// on-disk writes. It is unexported and not persisted.
	mu                sync.Mutex      `json:"-"`
	Port              int             `json:"port"`
	Host              string          `json:"host"`
	DataDir           string          `json:"dataDir"`
	DBPath            string          `json:"dbPath"`
	Workspaces        []string        `json:"workspaces"`
	Agents            []acp.AgentInfo `json:"agents"`
	TLSEnabled        bool            `json:"tlsEnabled"`
	TLSCertDir        string          `json:"tlsCertDir,omitempty"`
	PairingTTLSeconds int             `json:"pairingTtlSeconds,omitempty"`
	// CredentialInactivityTTLSeconds is the sliding-window inactivity expiry for
	// paired device credentials. A device that goes this long without a
	// successful authenticated request must re-pair. Sliding expiry is ON by
	// default for fresh installs (defaultCredentialInactivityTTLSeconds, 30
	// days). For existing config files that omit this field, the value loads as
	// 0 (disabled) — this is intentional to avoid silently re-enabling expiry
	// for users who may have relied on permanent credentials. Set this field
	// explicitly to enable expiry on an existing install. An explicit value of
	// 0 disables expiry entirely (credentials never expire).
	CredentialInactivityTTLSeconds int `json:"credentialInactivityTtlSeconds,omitempty"`
}

// defaultCredentialInactivityTTLSeconds is the default sliding-window credential
// inactivity expiry (30 days). It is applied to fresh configs so sliding expiry
// is on by default per the product decision, while an explicitly configured 0
// still disables expiry.
const defaultCredentialInactivityTTLSeconds = 2592000

// Default returns the default configuration.
//
// It derives the data directory from the current user's home directory. If the
// home directory cannot be determined, Default panics with a clear error
// rather than silently falling back to "." (which would write the data
// directory, SQLite database, TLS keys, and config.json into the current
// working directory — a surprising and divergent location across launches).
// Callers that prefer to handle the error should use DefaultOrError.
func Default() *Config {
	cfg, err := DefaultOrError()
	if err != nil {
		// Fail loudly: do not silently write app data to an arbitrary CWD.
		panic(fmt.Errorf("config: cannot build default config: %w", err))
	}
	return cfg
}

// DefaultOrError returns the default configuration or an error if the user's
// home directory cannot be determined. Use this instead of Default when the
// caller can surface the error to the user.
func DefaultOrError() (*Config, error) {
	homeDir, err := os.UserHomeDir()
	if err != nil {
		return nil, fmt.Errorf("determine user home directory: %w", err)
	}
	dataDir := filepath.Join(homeDir, ".local-agent")

	return &Config{
		Port:              7337,
		Host:              "0.0.0.0",
		DataDir:           dataDir,
		DBPath:            filepath.Join(dataDir, "local-agent.db"),
		Workspaces:        []string{},
		Agents:            []acp.AgentInfo{},
		TLSCertDir:        filepath.Join(dataDir, "tls"),
		PairingTTLSeconds: 300,

		CredentialInactivityTTLSeconds: defaultCredentialInactivityTTLSeconds,
	}, nil
}

// Load reads the config from ~/.local-agent/config.json.
// Returns Default() if the file doesn't exist.
func Load() (*Config, error) {
	homeDir, err := os.UserHomeDir()
	if err != nil {
		return nil, err
	}
	configPath := filepath.Join(homeDir, ".local-agent", "config.json")

	data, err := os.ReadFile(configPath) //nolint:gosec // configPath is constructed from the current user's home directory.
	if err != nil {
		if os.IsNotExist(err) {
			// No config file yet — return defaults. Home dir was already
			// resolved successfully above, so this cannot fail.
			return DefaultOrError()
		}
		return nil, err
	}

	var cfg Config
	if unmarshalErr := json.Unmarshal(data, &cfg); unmarshalErr != nil {
		return nil, unmarshalErr
	}

	// Fill in any missing defaults.
	def, err := DefaultOrError()
	if err != nil {
		return nil, err
	}
	if cfg.Port == 0 {
		cfg.Port = def.Port
	}
	if cfg.Host == "" {
		cfg.Host = def.Host
	}
	if cfg.DataDir == "" {
		cfg.DataDir = def.DataDir
	}
	if cfg.DBPath == "" {
		cfg.DBPath = def.DBPath
	}
	if cfg.Workspaces == nil {
		cfg.Workspaces = []string{}
	}
	if cfg.Agents == nil {
		cfg.Agents = []acp.AgentInfo{}
	}
	if cfg.TLSCertDir == "" {
		cfg.TLSCertDir = def.TLSCertDir
	}
	if cfg.PairingTTLSeconds == 0 {
		cfg.PairingTTLSeconds = def.PairingTTLSeconds
	}
	// CredentialInactivityTTLSeconds is intentionally NOT zero-filled from the
	// default here. With a plain int we cannot distinguish "field omitted" from
	// "explicitly set to 0", and 0 is a meaningful value (expiry disabled). A
	// fresh install with no config file receives the 30-day default via
	// DefaultOrError; an existing config file that omits the field loads as 0
	// (disabled) to avoid silently re-enabling expiry for users who may have
	// relied on permanent credentials. Set the field explicitly to enable
	// expiry on an existing install.

	return &cfg, nil
}

// Save writes the config to ~/.local-agent/config.json.
func (c *Config) Save() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.saveLocked()
}

// saveLocked writes the config to disk. The caller must hold c.mu.
func (c *Config) saveLocked() error {
	dir := filepath.Dir(filepath.Join(c.DataDir, "config.json"))
	if err := os.MkdirAll(dir, appDataDirPerm); err != nil {
		return err
	}

	data, err := json.MarshalIndent(c, "", "  ")
	if err != nil {
		return err
	}

	configPath := filepath.Join(c.DataDir, "config.json")
	return os.WriteFile(configPath, data, configFilePerm)
}

// UpsertAgent adds or updates an agent in the persisted config. It atomically
// (under the config mutex) replaces an existing agent with the same ID or
// appends a new one, then saves. Concurrent calls are safe.
func (c *Config) UpsertAgent(agent acp.AgentInfo) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	found := false
	for i, a := range c.Agents {
		if a.ID == agent.ID {
			c.Agents[i] = agent
			found = true
			break
		}
	}
	if !found {
		c.Agents = append(c.Agents, agent)
	}
	return c.saveLocked()
}

// DeleteAgent removes an agent from the persisted config by ID. It atomically
// (under the config mutex) re-slices and saves. Concurrent calls are safe.
func (c *Config) DeleteAgent(id string) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	for i, a := range c.Agents {
		if a.ID == id {
			c.Agents = append(c.Agents[:i], c.Agents[i+1:]...)
			break
		}
	}
	return c.saveLocked()
}

// RemoveWorkspacePath drops the given absolute path from the Workspaces list
// and persists the updated config. It returns an error if the path was not
// registered. The caller is responsible for unregistering the workspace from
// the in-memory workspace manager before calling this.
func (c *Config) RemoveWorkspacePath(absPath string) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	found := false
	updated := make([]string, 0, len(c.Workspaces))
	for _, ws := range c.Workspaces {
		if ws == absPath {
			found = true
			continue
		}
		updated = append(updated, ws)
	}
	if !found {
		return fmt.Errorf("workspace not registered: %s", absPath)
	}
	c.Workspaces = updated
	return c.saveLocked()
}
