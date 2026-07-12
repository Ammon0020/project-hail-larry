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
	mu         sync.Mutex      `json:"-"`
	Port       int             `json:"port"`
	Host       string          `json:"host"`
	DataDir    string          `json:"dataDir"`
	DBPath     string          `json:"dbPath"`
	Workspaces []string        `json:"workspaces"`
	Agents     []acp.AgentInfo `json:"agents"`
	TLSEnabled bool            `json:"tlsEnabled"`
	TLSCertDir string          `json:"tlsCertDir,omitempty"`
	// HTTPSPort is the TCP port the HTTPS listener binds to when TLSEnabled is
	// true (dual HTTP+HTTPS mode). A value of 0 means "Port + 1" at runtime —
	// e.g. Port=7337 → HTTPS on 7338. Set explicitly to override. HTTP always
	// listens on Port regardless of this field, so users can pick a scheme by
	// typing http://IP:Port or https://IP:HTTPSPort in the browser without
	// restarting the daemon or flipping config.
	HTTPSPort         int `json:"httpsPort,omitempty"`
	PairingTTLSeconds int `json:"pairingTtlSeconds,omitempty"`
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
	// AllowRemoteWorkspaceRegistration controls whether paired devices may
	// register new workspace directories from the web UI / remote API. It
	// defaults to false: workspaces are registered from the host via the
	// `app add-folder <path>` CLI command, which keeps the sensitive surface
	// (a remote device could otherwise register ~/.ssh, /etc, etc. and read
	// files) on the host. Set to true to allow remote registration, in which
	// case the registration still goes through the grace-period pending
	// action flow (see RevocationGracePeriodSeconds) so other devices can
	// cancel a suspicious registration.
	AllowRemoteWorkspaceRegistration bool `json:"allowRemoteWorkspaceRegistration,omitempty"`
	// RevocationGracePeriodSeconds is the grace period (in seconds) that a
	// device revocation or workspace registration spends in a pending state
	// before being executed. During the grace period any connected device can
	// cancel the action — this protects a user whose device is stolen: their
	// other devices see the pending action via the broadcast event and can
	// cancel it before it takes effect. Defaults to 300 (5 minutes). A value
	// of 0 means instant execution (no grace period), preserving backward
	// compatibility for users who explicitly opt out of the grace window.
	RevocationGracePeriodSeconds int `json:"revocationGracePeriodSeconds,omitempty"`
}

// defaultCredentialInactivityTTLSeconds is the default sliding-window credential
// inactivity expiry (30 days). It is applied to fresh configs so sliding expiry
// is on by default per the product decision, while an explicitly configured 0
// still disables expiry.
const defaultCredentialInactivityTTLSeconds = 2592000

// defaultRevocationGracePeriodSeconds is the default grace period (5 minutes)
// applied to a fresh config and to legacy config files that omit the
// revocationGracePeriodSeconds key. An explicit 0 in the config disables the
// grace period (instant execution) and is respected.
const defaultRevocationGracePeriodSeconds = 300

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
		Port:       7337,
		Host:       "0.0.0.0",
		DataDir:    dataDir,
		DBPath:     filepath.Join(dataDir, "local-agent.db"),
		Workspaces: []string{},
		Agents:     []acp.AgentInfo{},
		// TLSEnabled defaults to true so fresh installs are secure by default —
		// device Bearer tokens must not travel in cleartext over the LAN. When
		// true the daemon runs in dual HTTP+HTTPS mode: HTTP on Port (cleartext
		// for LAN home use) AND HTTPS on HTTPSPort (Port+1 by default) for
		// coffee-shop TLS. An existing config file that explicitly sets
		// "tlsEnabled": false is respected (see Load) and disables the HTTPS
		// listener entirely (HTTP only); an older config file that omits the
		// field entirely also gets true (secure-by-default upgrade), unless
		// the user explicitly opts out.
		TLSEnabled:        true,
		TLSCertDir:        filepath.Join(dataDir, "tls"),
		PairingTTLSeconds: 300,

		CredentialInactivityTTLSeconds: defaultCredentialInactivityTTLSeconds,

		// Workspaces are registered from the host CLI by default; remote
		// registration is gated behind a grace-period pending action and is
		// off unless the user explicitly enables it.
		AllowRemoteWorkspaceRegistration: false,
		// Default 5-minute grace period for destructive actions (device
		// revocation, remote workspace registration) so a stolen device
		// cannot lock the user out or register sensitive directories before
		// the user's other devices can cancel.
		RevocationGracePeriodSeconds: defaultRevocationGracePeriodSeconds,
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

	// Detect whether the "tlsEnabled" key was explicitly present in the JSON.
	// A plain bool zero-fills to false on omission, which we cannot distinguish
	// from an explicit "tlsEnabled": false. To be secure-by-default on upgrade
	// (older config files predate the field) while still respecting an explicit
	// opt-out, decode into a raw map and check key presence: if the key is
	// absent, force TLS on; if present, the value decoded above stands.
	var raw map[string]json.RawMessage
	if unmarshalErr := json.Unmarshal(data, &raw); unmarshalErr != nil {
		return nil, unmarshalErr
	}
	if _, ok := raw["tlsEnabled"]; !ok {
		cfg.TLSEnabled = true
	}

	// Detect whether "revocationGracePeriodSeconds" was explicitly present in
	// the JSON, mirroring the tlsEnabled handling above. A plain int zero-fills
	// to 0 on omission, which we cannot distinguish from an explicit 0 (which
	// means "instant execution, no grace period"). For legacy config files
	// that predate the field, default to the 5-minute grace window so the
	// grace-period protection is on by default on upgrade; an explicit 0
	// remains respected as an opt-out.
	if _, ok := raw["revocationGracePeriodSeconds"]; !ok {
		cfg.RevocationGracePeriodSeconds = defaultRevocationGracePeriodSeconds
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
