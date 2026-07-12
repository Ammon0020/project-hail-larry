// Package mcp health checking: on-demand health verification for configured
// MCP servers.
//
// CheckHealth performs parallel connectivity checks for all servers in the
// config file. stdio servers are verified via exec.LookPath (binary exists?);
// http/sse servers are verified via a TCP dial to the URL's host:port. The
// results are returned as a []ServerStatus ready for JSON serialization by
// the REST handler.

package mcp

import (
	"fmt"
	"net"
	"net/url"
	"os/exec"
	"sort"
	"strings"
	"sync"
	"time"
)

// defaultHealthTimeout is the per-server timeout for health checks. Short
// enough to keep the API responsive; long enough for a local stdio LookPath
// or a nearby TCP dial.
const defaultHealthTimeout = 2 * time.Second

// Health status values returned by CheckHealth / checkServer.
const (
	statusHealthy   = "healthy"
	statusUnhealthy = "unhealthy"
	statusDisabled  = "disabled"
	statusUnknown   = "unknown"
)

// errNoCommand is the error string returned when a stdio server has no command
// configured. Extracted as a constant to satisfy goconst (3+ occurrences across
// health.go and its tests).
const errNoCommand = "no command configured"

// ServerStatus represents the health of a single MCP server.
type ServerStatus struct {
	Name    string `json:"name"`
	Enabled bool   `json:"enabled"`
	Status  string `json:"status"` // healthy, unhealthy, disabled, unknown
	Error   string `json:"error,omitempty"`
}

// CheckHealth performs on-demand health checks for all configured MCP servers.
// Each enabled server is checked in parallel; disabled servers are reported as
// statusDisabled without performing any check. The timeout parameter caps
// individual checks; pass 0 to use the default (2s).
//
// Check logic per transport type:
//   - stdio (or empty type): exec.LookPath on the command binary. Found →
//     healthy; not found → unhealthy.
//   - http / sse: TCP dial to the URL's host:port. Connected → healthy;
//     timeout/refused → unhealthy.
func CheckHealth(f *File, timeout time.Duration) []ServerStatus {
	if f == nil || len(f.McpServers) == 0 {
		return nil
	}
	if timeout <= 0 {
		timeout = defaultHealthTimeout
	}

	// Collect server names in sorted order for deterministic output.
	names := make([]string, 0, len(f.McpServers))
	for name := range f.McpServers {
		names = append(names, name)
	}
	sort.Strings(names)

	results := make([]ServerStatus, len(names))
	var wg sync.WaitGroup

	for i, name := range names {
		cfg := f.McpServers[name]
		results[i].Name = name
		results[i].Enabled = cfg.Enabled == nil || *cfg.Enabled

		if !results[i].Enabled {
			// Disabled servers skip the check entirely.
			results[i].Status = statusDisabled
			continue
		}

		wg.Add(1)
		go func(idx int, serverCfg ServerConfig) {
			defer wg.Done()
			results[idx].Status, results[idx].Error = checkServer(serverCfg, timeout)
		}(i, cfg)
	}

	wg.Wait()
	return results
}

// checkServer performs a single health check based on the server's transport
// type. Returns (status, errorMessage). Uses effectiveType so a server with no
// `type` field but a `url` (e.g. context7) is checked as HTTP, not stdio.
func checkServer(cfg ServerConfig, timeout time.Duration) (string, string) {
	switch effectiveType(cfg) {
	case transportTypeStdio:
		return checkStdio(cfg)
	case transportTypeHTTP, transportTypeSSE:
		return checkNetwork(cfg, timeout)
	default:
		return statusUnknown, fmt.Sprintf("unsupported transport type: %s", cfg.Type)
	}
}

// checkStdio verifies a stdio MCP server by checking if its command binary
// exists on PATH. This is fast and has no side effects (no process is spawned).
func checkStdio(cfg ServerConfig) (string, string) {
	if cfg.Command == "" {
		return statusUnhealthy, errNoCommand
	}
	// Expand env vars in the command before looking it up, matching the
	// expansion that happens at session-start time in ToACP.
	cmd := expandEnv(cfg.Command)
	if _, err := exec.LookPath(cmd); err != nil {
		return statusUnhealthy, fmt.Sprintf("executable not found: %s", cmd)
	}
	return statusHealthy, ""
}

// checkNetwork verifies an http/sse MCP server by performing a TCP dial to
// the URL's host:port. This confirms the host is reachable and listening
// without sending any HTTP traffic (which might trigger auth failures or
// side effects on some servers).
func checkNetwork(cfg ServerConfig, timeout time.Duration) (string, string) {
	if cfg.URL == "" {
		return statusUnhealthy, "no URL configured"
	}
	// Expand env vars in the URL before parsing, matching ToACP behavior.
	rawURL := expandEnv(cfg.URL)
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return statusUnhealthy, fmt.Sprintf("invalid URL: %s", err)
	}

	host := parsed.Hostname()
	port := parsed.Port()
	if port == "" {
		// Default ports based on scheme.
		switch strings.ToLower(parsed.Scheme) {
		case "https", "wss":
			port = "443"
		default:
			port = "80"
		}
	}

	addr := net.JoinHostPort(host, port)
	conn, err := net.DialTimeout("tcp", addr, timeout)
	if err != nil {
		return statusUnhealthy, fmt.Sprintf("connection failed: %s", err)
	}
	_ = conn.Close()
	return statusHealthy, ""
}
