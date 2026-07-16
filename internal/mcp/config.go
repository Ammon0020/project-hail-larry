// Package mcp manages persistent MCP (Model Context Protocol) server
// configuration for the Local Agent Interface.
//
// Config is stored as a Claude Desktop–compatible `mcpServers` map wrapped in a
// small envelope that carries `enabled` flags and a schema version. The file
// lives at `~/.local-agent/mcp.json` (the same base dir as config.json). On the
// wire to ACP we translate each enabled entry to an `acp.McpServer`, expanding
// `${VAR}` environment references against `os.Getenv` at that point so secrets
// can be kept out of the config file.
//
// Blueprint references: docs/research/mcp-config-design.md.
package mcp

import (
	"encoding/json"
	"fmt"
	"os"
	"regexp"
	"sort"
	"strings"

	"github.com/coder/acp-go-sdk"

	"github.com/adama/local-agent/internal/fsutil"
)

const (
	// configFilePerm matches internal/config: the file may contain secrets, so
	// it is readable/writable only by the owner.
	configFilePerm = 0o600
	// CurrentVersion is the on-disk envelope version this build understands.
	// Bump (and add a migration) when the envelope shape changes.
	CurrentVersion = 1

	// MCP transport type identifiers (ServerConfig.Type values and the
	// corresponding ACP wire-shape tags). Centralised so the case branches in
	// ToACP and the capability filter stay in sync.
	transportTypeHTTP  = "http"
	transportTypeSSE   = "sse"
	transportTypeStdio = "stdio"
)

// ServerConfig is our on-disk representation of one MCP server entry. It is a
// superset of the Claude Desktop / Cursor / Windsurf shape: the per-server
// object is byte-for-byte copy/paste compatible with those editors, and we add
// an optional `enabled` flag (nil => enabled) plus optional `cwd` for stdio.
//
// The `Env` and `Headers` fields use the Claude-style `{"K":"V"}` map shape on
// disk; translation to ACP's `[]EnvVariable` / `[]HttpHeader` array shape
// happens in ToACP at session-start time.
type ServerConfig struct {
	// Type selects the transport. "http" | "sse" | "stdio". Empty defaults to
	// "stdio" (the Claude Desktop convention: no `type` field means stdio).
	Type    string            `json:"type,omitempty"`
	Command string            `json:"command,omitempty"` // stdio
	Args    []string          `json:"args,omitempty"`    // stdio
	Env     map[string]string `json:"env,omitempty"`     // stdio (Claude-style map)
	Cwd     string            `json:"cwd,omitempty"`     // stdio (optional)
	URL     string            `json:"url,omitempty"`     // http/sse
	Headers map[string]string `json:"headers,omitempty"` // http/sse
	// Enabled gates whether the server is sent to ACP. nil => enabled (default
	// on, so a freshly pasted Claude Desktop config — which omits this field —
	// activates every entry without the user having to flip each one).
	Enabled *bool `json:"enabled,omitempty"`
}

// File is the on-disk envelope for mcp.json.
//
//   - `$schema` powers editor autocomplete for users who open the file in VS Code.
//   - `version` lets us migrate the envelope shape later.
//   - `mcpServers` is the Claude Desktop–compatible map keyed by server name.
type File struct {
	Schema     string                  `json:"$schema,omitempty"`
	Version    int                     `json:"version"`
	McpServers map[string]ServerConfig `json:"mcpServers"`
}

// NewFile returns an empty, valid File (Version set, McpServers initialized)
// suitable for first-write or for callers that want a non-nil placeholder.
func NewFile() *File {
	return &File{
		Version:    CurrentVersion,
		McpServers: map[string]ServerConfig{},
	}
}

// Load reads the MCP config from the given path. A missing file is not an
// error: an empty File (Version=1, empty McpServers map) is returned so
// callers can treat "no config yet" and "empty config" uniformly. Any other
// read or parse error is returned to the caller.
func Load(path string) (*File, error) {
	data, err := os.ReadFile(path) //nolint:gosec // path is constructed by the caller from a trusted base dir.
	if err != nil {
		if os.IsNotExist(err) {
			return NewFile(), nil
		}
		return nil, fmt.Errorf("read mcp config: %w", err)
	}

	f := NewFile()
	if err := json.Unmarshal(data, f); err != nil {
		return nil, fmt.Errorf("parse mcp config: %w", err)
	}
	if f.McpServers == nil {
		f.McpServers = map[string]ServerConfig{}
	}
	return f, nil
}

// Save writes the MCP config to the given path atomically (temp file + rename)
// so a crashed write never leaves a half-written config. The parent directory
// is created with mode 0700 if missing, and the file is written with mode 0600
// because it may contain secrets (env values, bearer tokens).
func Save(path string, f *File) error {
	if f == nil {
		return fmt.Errorf("save mcp config: nil file")
	}
	if f.McpServers == nil {
		// Avoid serializing `"mcpServers":null` — round-trip should be stable.
		f.McpServers = map[string]ServerConfig{}
	}
	if f.Version == 0 {
		f.Version = CurrentVersion
	}

	data, err := json.MarshalIndent(f, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal mcp config: %w", err)
	}

	if err := fsutil.WriteFileAtomic(path, data, configFilePerm); err != nil {
		return fmt.Errorf("write mcp config: %w", err)
	}
	return nil
}

// Enabled returns a map containing only the enabled servers from f. A server
// with Enabled == nil is treated as enabled (the default-on convention), so a
// freshly pasted Claude Desktop config activates every entry. Servers with
// Enabled == &false are dropped.
func Enabled(f *File) map[string]ServerConfig {
	out := make(map[string]ServerConfig, len(f.McpServers))
	for name, cfg := range f.McpServers {
		if cfg.Enabled != nil && !*cfg.Enabled {
			continue
		}
		out[name] = cfg
	}
	return out
}

// envVarRe matches ${VAR} references for env expansion. It matches the longest
// legal env-var name (letters, digits, underscore; must start with a letter or
// underscore) so ${API_KEY}_suffix is expanded correctly.
var envVarRe = regexp.MustCompile(`\$\{([A-Za-z_][A-Za-z0-9_]*)\}`)

// expandEnv replaces ${VAR} references in s with os.Getenv(VAR). Unknown
// variables expand to the empty string (matching Claude Code/Cursor behavior).
// This is applied at session-start time so secrets can be kept out of mcp.json.
func expandEnv(s string) string {
	return envVarRe.ReplaceAllStringFunc(s, func(m string) string {
		name := m[2 : len(m)-1] // strip ${ and }
		return os.Getenv(name)
	})
}

// effectiveType returns the transport type to use for a server, inferring it
// from the configured fields when cfg.Type is empty. The Claude Desktop
// convention is that an absent `type` field means stdio — but that convention
// only holds when a `command` is present. A server configured with a `url` and
// no `command` (a common shape for remote MCP servers like context7) is
// unambiguously an HTTP server, so we infer "http" in that case to avoid
// misclassifying it as a stdio server with an empty command.
func effectiveType(cfg ServerConfig) string {
	t := strings.ToLower(cfg.Type)
	if t != "" {
		return t
	}
	// Empty type: infer from fields. URL without Command => http; otherwise stdio.
	if cfg.URL != "" && cfg.Command == "" {
		return transportTypeHTTP
	}
	return transportTypeStdio
}

// ToACP translates one ServerConfig to an acp.McpServer ready to be sent on
// session/new or session/load. The transport is selected by effectiveType
// (which infers from cfg.Type, falling back to field-based inference when it's
// empty):
//
//   - "http"  => McpServerHttpInline (requires mcp_capabilities.http on the agent)
//   - "sse"   => McpServerSseInline  (requires mcp_capabilities.sse on the agent)
//   - "stdio" => McpServerStdio (always supported)
//
// ${VAR} env references in Command, Args, Env values, Url, and Header values
// are expanded against os.Getenv at this point. The Claude-style Env map and
// Headers map are translated to ACP's []EnvVariable / []HttpHeader array shape.
// Returns an error for an unknown Type.
func ToACP(name string, cfg ServerConfig) (acp.McpServer, error) {
	switch effectiveType(cfg) {
	case "stdio":
		env := make([]acp.EnvVariable, 0, len(cfg.Env))
		// Sort keys for deterministic wire output (stable diffs in tests/logs).
		keys := make([]string, 0, len(cfg.Env))
		for k := range cfg.Env {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		for _, k := range keys {
			env = append(env, acp.EnvVariable{
				Name:  k,
				Value: expandEnv(cfg.Env[k]),
			})
		}
		args := make([]string, len(cfg.Args))
		for i, a := range cfg.Args {
			args[i] = expandEnv(a)
		}
		return acp.McpServer{
			Stdio: &acp.McpServerStdio{
				Name:    name,
				Command: expandEnv(cfg.Command),
				Args:    args,
				Env:     env,
			},
		}, nil

	case transportTypeHTTP:
		return acp.McpServer{
			Http: &acp.McpServerHttpInline{
				Name:    name,
				Type:    transportTypeHTTP,
				Url:     expandEnv(cfg.URL),
				Headers: headersToACP(cfg.Headers),
			},
		}, nil

	case transportTypeSSE:
		return acp.McpServer{
			Sse: &acp.McpServerSseInline{
				Name:    name,
				Type:    transportTypeSSE,
				Url:     expandEnv(cfg.URL),
				Headers: headersToACP(cfg.Headers),
			},
		}, nil

	default:
		return acp.McpServer{}, fmt.Errorf("mcp server %q: unknown type %q (want http, sse, or stdio)", name, cfg.Type)
	}
}

// headersToACP translates a Claude-style {name: value} headers map to ACP's
// []HttpHeader array shape, expanding ${VAR} references in values. Keys are
// sorted for deterministic wire output.
func headersToACP(headers map[string]string) []acp.HttpHeader {
	out := make([]acp.HttpHeader, 0, len(headers))
	keys := make([]string, 0, len(headers))
	for k := range headers {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		out = append(out, acp.HttpHeader{
			Name:  k,
			Value: expandEnv(headers[k]),
		})
	}
	return out
}

// ToACPSlice translates every enabled server in f to an []acp.McpServer,
// filtering by the agent's advertised McpCapabilities. stdio servers are
// always included (the ACP spec requires every agent to support stdio). http
// servers are dropped unless caps.Http is true; sse servers are dropped unless
// caps.Sse is true. Servers are emitted in deterministic (sorted-by-name) order
// so the wire payload is stable across runs.
//
// Errors from individual ToACP translations are aggregated: a single bad entry
// (e.g. unknown type) causes the whole call to fail rather than silently
// dropping a server the user configured. The caller should log the error and
// fall back to an empty server list rather than sending a partial one.
func ToACPSlice(f *File, caps acp.McpCapabilities) ([]acp.McpServer, error) {
	enabled := Enabled(f)
	names := make([]string, 0, len(enabled))
	for name := range enabled {
		names = append(names, name)
	}
	sort.Strings(names)

	out := make([]acp.McpServer, 0, len(names))
	for _, name := range names {
		cfg := enabled[name]
		switch effectiveType(cfg) {
		case transportTypeHTTP:
			if !caps.Http {
				continue
			}
		case transportTypeSSE:
			if !caps.Sse {
				continue
			}
		case transportTypeStdio:
			// Always supported per ACP spec.
		default:
			return nil, fmt.Errorf("mcp server %q: unknown type %q", name, cfg.Type)
		}
		srv, err := ToACP(name, cfg)
		if err != nil {
			return nil, err
		}
		out = append(out, srv)
	}
	return out, nil
}
