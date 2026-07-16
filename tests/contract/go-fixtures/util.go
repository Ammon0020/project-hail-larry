// Package main: shared helpers for the contract fixture harness.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"time"
)

// keepState, when true, leaves the isolated state dir in place after the run
// for debugging. Set via the -keep-state flag.
var keepState bool

// writeJSONFile writes v as pretty-printed JSON to path. Parent directories
// must already exist.
func writeJSONFile(path string, v any) error {
	data, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		return err
	}
	data = append(data, '\n')
	return os.WriteFile(path, data, 0o644)
}

// writeJSONLFile writes frames as JSON Lines (one JSON object per line) to
// path. Each line is compact JSON; the file ends with a trailing newline.
func writeJSONLFile(path string, frames []wsFrame) error {
	var b []byte
	for _, f := range frames {
		line, err := json.Marshal(f)
		if err != nil {
			return fmt.Errorf("marshal ws frame: %w", err)
		}
		b = append(b, line...)
		b = append(b, '\n')
	}
	return os.WriteFile(path, b, 0o644)
}

// jsonUnmarshal is a thin wrapper around json.Unmarshal so callers in cli.go
// can decode without importing encoding/json directly.
func jsonUnmarshal(data []byte, v any) error {
	return json.Unmarshal(data, v)
}

// writeSeedConfigWithPort is like writeSeedConfig but overrides the port so
// the CLI's localhost:<port> URL hits the in-process httptest server.
func writeSeedConfigWithPort(stateDir, seedWsPath string, port int) error {
	cfg := map[string]any{
		"port":                           port,
		"host":                           "127.0.0.1",
		"dataDir":                        stateDir,
		"dbPath":                         filepath.Join(stateDir, "local-agent.db"),
		"workspaces":                     []string{seedWsPath},
		"agents":                         json.RawMessage(seedAgentJSON),
		"tlsEnabled":                     false,
		"pairingTtlSeconds":              300,
		"revocationGracePeriodSeconds":   300,
		"credentialInactivityTtlSeconds": 2592000,
	}
	data, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(stateDir, "config.json"), data, 0o600)
}

// _ = time.Time ensures the time import stays used even if timeoutAfter is the
// only caller and lint rules vary; it is compiled away.
var _ = time.Second
