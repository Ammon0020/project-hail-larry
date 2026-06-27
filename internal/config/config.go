// Package config manages persistent configuration for the Local Agent Interface.
// Config is stored in ~/.local-agent/config.json.
// Blueprint references: Sec 20 (Configuration).
package config

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/adama/local-agent/internal/acp"
)

const (
	appDataDirPerm = 0700
	configFilePerm = 0600
)

// Config is the persistent application configuration.
type Config struct {
	Port              int             `json:"port"`
	Host              string          `json:"host"`
	DataDir           string          `json:"dataDir"`
	DBPath            string          `json:"dbPath"`
	Workspaces        []string        `json:"workspaces"`
	Agents            []acp.AgentInfo `json:"agents"`
	TLSEnabled        bool            `json:"tlsEnabled"`
	TLSCertDir        string          `json:"tlsCertDir,omitempty"`
	PairingTTLSeconds int             `json:"pairingTtlSeconds,omitempty"`
}

// Default returns the default configuration.
func Default() *Config {
	homeDir, err := os.UserHomeDir()
	if err != nil {
		homeDir = "."
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
	}
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
			return Default(), nil
		}
		return nil, err
	}

	var cfg Config
	if err := json.Unmarshal(data, &cfg); err != nil {
		return nil, err
	}

	// Fill in any missing defaults.
	def := Default()
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

	return &cfg, nil
}

// Save writes the config to ~/.local-agent/config.json.
func (c *Config) Save() error {
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

// RemoveWorkspacePath drops the given absolute path from the Workspaces list
// and persists the updated config. It returns an error if the path was not
// registered. The caller is responsible for unregistering the workspace from
// the in-memory workspace manager before calling this.
func (c *Config) RemoveWorkspacePath(absPath string) error {
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
	return c.Save()
}
