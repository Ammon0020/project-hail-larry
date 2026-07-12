package mcp

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"

	"github.com/coder/acp-go-sdk"
)

// boolPtr returns a pointer to b, used to set ServerConfig.Enabled in tests.
func boolPtr(b bool) *bool { return &b }

// TestLoadMissingFileReturnsEmpty verifies Load on a non-existent path returns
// a valid empty File (not nil) and no error, so callers can treat "no config
// yet" and "empty config" uniformly.
func TestLoadMissingFileReturnsEmpty(t *testing.T) {
	t.Parallel()
	path := filepath.Join(t.TempDir(), "does-not-exist.json")
	f, err := Load(path)
	if err != nil {
		t.Fatalf("Load missing file: unexpected error: %v", err)
	}
	if f == nil {
		t.Fatal("Load missing file: returned nil File")
	}
	if f.Version != CurrentVersion {
		t.Errorf("Load missing file: Version = %d, want %d", f.Version, CurrentVersion)
	}
	if f.McpServers == nil {
		t.Error("Load missing file: McpServers is nil, want empty map")
	}
	if len(f.McpServers) != 0 {
		t.Errorf("Load missing file: %d servers, want 0", len(f.McpServers))
	}
}

// TestSaveLoadRoundTrip verifies Save then Load preserves the envelope: schema,
// version, and every server field including the Enabled pointer.
func TestSaveLoadRoundTrip(t *testing.T) {
	t.Parallel()
	path := filepath.Join(t.TempDir(), "subdir", "mcp.json") // subdir forces MkdirAll
	original := &File{
		Schema:  "https://local-agent.dev/schemas/mcp.json",
		Version: CurrentVersion,
		McpServers: map[string]ServerConfig{
			"github": {
				Command: "npx",
				Args:    []string{"-y", "@modelcontextprotocol/server-github"},
				Env:     map[string]string{"GITHUB_TOKEN": "ghp_abc"},
				Enabled: boolPtr(true),
			},
			"linear": {
				Type:    "http",
				URL:     "https://mcp.linear.app/mcp",
				Headers: map[string]string{"Authorization": "Bearer xyz"},
				Enabled: boolPtr(false),
			},
			"default-on": {
				// Enabled nil => default on; must round-trip as nil (not &true).
				Command: "echo",
				Args:    []string{"hi"},
			},
		},
	}

	if err := Save(path, original); err != nil {
		t.Fatalf("Save: %v", err)
	}

	loaded, err := Load(path)
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if loaded.Schema != original.Schema {
		t.Errorf("Schema round-trip: got %q want %q", loaded.Schema, original.Schema)
	}
	if loaded.Version != original.Version {
		t.Errorf("Version round-trip: got %d want %d", loaded.Version, original.Version)
	}
	if !reflect.DeepEqual(loaded.McpServers, original.McpServers) {
		t.Errorf("McpServers round-trip mismatch:\n got = %+v\nwant = %+v", loaded.McpServers, original.McpServers)
	}

	// File mode must be 0600 — the file may contain secrets.
	info, err := os.Stat(path)
	if err != nil {
		t.Fatalf("Stat: %v", err)
	}
	if mode := info.Mode().Perm(); mode != configFilePerm {
		t.Errorf("File mode = %o, want %o", mode, configFilePerm)
	}
}

// TestSaveAtomicNoTempLeft verifies Save does not leave temp files behind on
// success (the temp file is renamed into place).
func TestSaveAtomicNoTempLeft(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	path := filepath.Join(dir, "mcp.json")
	if err := Save(path, NewFile()); err != nil {
		t.Fatalf("Save: %v", err)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatalf("ReadDir: %v", err)
	}
	for _, e := range entries {
		if e.Name() != "mcp.json" {
			t.Errorf("leftover temp file in dir: %s", e.Name())
		}
	}
}

// TestEnabledFiltering verifies Enabled() includes servers with enabled=true and
// enabled=nil (default-on) and excludes servers with enabled=false.
func TestEnabledFiltering(t *testing.T) {
	t.Parallel()
	f := &File{
		Version: CurrentVersion,
		McpServers: map[string]ServerConfig{
			"on":      {Command: "a", Enabled: boolPtr(true)},
			"off":     {Command: "b", Enabled: boolPtr(false)},
			"default": {Command: "c"}, // nil => enabled
		},
	}
	got := Enabled(f)
	want := map[string]ServerConfig{
		"on":      {Command: "a", Enabled: boolPtr(true)},
		"default": {Command: "c"},
	}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("Enabled() = %+v, want %+v", got, want)
	}
}

// TestToACPStdio verifies ToACP for a stdio server translates the Claude-style
// Env map to ACP's []EnvVariable array and sets the stdio transport.
func TestToACPStdio(t *testing.T) {
	t.Parallel()
	cfg := ServerConfig{
		Command: "npx",
		Args:    []string{"-y", "pkg"},
		Env:     map[string]string{"A": "1", "B": "2"},
	}
	srv, err := ToACP("github", cfg)
	if err != nil {
		t.Fatalf("ToACP: %v", err)
	}
	if srv.Stdio == nil {
		t.Fatal("expected Stdio transport, got nil")
	}
	if srv.Http != nil || srv.Sse != nil {
		t.Errorf("expected only Stdio set, got Http=%v Sse=%v", srv.Http, srv.Sse)
	}
	if srv.Stdio.Name != "github" {
		t.Errorf("Name = %q, want github", srv.Stdio.Name)
	}
	if srv.Stdio.Command != "npx" {
		t.Errorf("Command = %q, want npx", srv.Stdio.Command)
	}
	if !reflect.DeepEqual(srv.Stdio.Args, []string{"-y", "pkg"}) {
		t.Errorf("Args = %v, want [-y pkg]", srv.Stdio.Args)
	}
	// Env must be sorted by name for deterministic output.
	got := make(map[string]string, len(srv.Stdio.Env))
	for _, e := range srv.Stdio.Env {
		got[e.Name] = e.Value
	}
	want := map[string]string{"A": "1", "B": "2"}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("Env = %v, want %v", got, want)
	}
	// Verify sorted order.
	names := make([]string, 0, len(srv.Stdio.Env))
	for _, e := range srv.Stdio.Env {
		names = append(names, e.Name)
	}
	if !sort.StringsAreSorted(names) {
		t.Errorf("Env not sorted by name: %v", names)
	}
}

// TestToACPHttp verifies ToACP for an http server produces McpServerHttpInline
// with Type="http" and headers translated to []HttpHeader.
func TestToACPHttp(t *testing.T) {
	t.Parallel()
	cfg := ServerConfig{
		Type:    "http",
		URL:     "https://mcp.example.com/mcp",
		Headers: map[string]string{"Authorization": "Bearer tok"},
	}
	srv, err := ToACP("remote", cfg)
	if err != nil {
		t.Fatalf("ToACP: %v", err)
	}
	if srv.Http == nil {
		t.Fatal("expected Http transport, got nil")
	}
	if srv.Stdio != nil || srv.Sse != nil {
		t.Errorf("expected only Http set, got Stdio=%v Sse=%v", srv.Stdio, srv.Sse)
	}
	if srv.Http.Name != "remote" {
		t.Errorf("Name = %q, want remote", srv.Http.Name)
	}
	if srv.Http.Type != "http" {
		t.Errorf("Type = %q, want http", srv.Http.Type)
	}
	if srv.Http.Url != "https://mcp.example.com/mcp" {
		t.Errorf("Url = %q", srv.Http.Url)
	}
	if len(srv.Http.Headers) != 1 {
		t.Fatalf("Headers len = %d, want 1", len(srv.Http.Headers))
	}
	h := srv.Http.Headers[0]
	if h.Name != "Authorization" || h.Value != "Bearer tok" {
		t.Errorf("Header = {%q: %q}, want {Authorization: Bearer tok}", h.Name, h.Value)
	}
}

// TestToACPSse verifies ToACP for an sse server produces McpServerSseInline.
func TestToACPSse(t *testing.T) {
	t.Parallel()
	cfg := ServerConfig{Type: "sse", URL: "https://mcp.example.com/sse"}
	srv, err := ToACP("s", cfg)
	if err != nil {
		t.Fatalf("ToACP: %v", err)
	}
	if srv.Sse == nil {
		t.Fatal("expected Sse transport, got nil")
	}
	if srv.Sse.Type != "sse" {
		t.Errorf("Type = %q, want sse", srv.Sse.Type)
	}
	if srv.Sse.Url != "https://mcp.example.com/sse" {
		t.Errorf("Url = %q", srv.Sse.Url)
	}
}

// TestToACPUnknownType verifies ToACP returns an error for an unrecognized Type.
func TestToACPUnknownType(t *testing.T) {
	t.Parallel()
	_, err := ToACP("bad", ServerConfig{Type: "ftp"})
	if err == nil {
		t.Fatal("expected error for unknown type, got nil")
	}
}

// TestToACPUrlWithoutTypeInferredAsHttp verifies that a server with a `url` but
// no `type` field (and no `command`) is inferred as HTTP — the context7 shape.
// Without this inference, the server would be sent as stdio with an empty
// command, which fails at runtime.
func TestToACPUrlWithoutTypeInferredAsHttp(t *testing.T) {
	t.Parallel()
	cfg := ServerConfig{
		URL:     "https://mcp.context7.com/mcp",
		Headers: map[string]string{"CONTEXT7_API_KEY": "tok"},
	}
	srv, err := ToACP("context7", cfg)
	if err != nil {
		t.Fatalf("ToACP: %v", err)
	}
	if srv.Http == nil {
		t.Fatal("expected Http transport (inferred from URL), got nil")
	}
	if srv.Stdio != nil {
		t.Errorf("expected no Stdio transport, got %v", srv.Stdio)
	}
	if srv.Http.Url != "https://mcp.context7.com/mcp" {
		t.Errorf("Url = %q", srv.Http.Url)
	}
}

// TestToACPEnvExpansion verifies ${VAR} references are expanded against
// os.Getenv at translation time in Command, Args, Env values, Url, and Headers.
//
// This test cannot run with t.Parallel because t.Setenv mutates the process
// environment, which must be serialized with other tests.
func TestToACPEnvExpansion(t *testing.T) {
	t.Setenv("MCP_CMD", "mycmd")
	t.Setenv("MCP_ARG", "myarg")
	t.Setenv("MCP_VAL", "myval")
	t.Setenv("MCP_URL", "https://example.com")
	t.Setenv("MCP_HDR", "tok123")

	cfg := ServerConfig{
		Command: "$MCP_CMD", // not expanded — only ${VAR} form is supported
		Args:    []string{"${MCP_ARG}"},
		Env:     map[string]string{"K": "${MCP_VAL}"},
	}
	srv, err := ToACP("e", cfg)
	if err != nil {
		t.Fatalf("ToACP: %v", err)
	}
	if srv.Stdio.Command != "$MCP_CMD" {
		t.Errorf("Command: $VAR form should NOT expand; got %q", srv.Stdio.Command)
	}
	if len(srv.Stdio.Args) != 1 || srv.Stdio.Args[0] != "myarg" {
		t.Errorf("Args[0] = %q, want myarg", srv.Stdio.Args[0])
	}
	if len(srv.Stdio.Env) != 1 || srv.Stdio.Env[0].Value != "myval" {
		t.Errorf("Env[0].Value = %q, want myval", srv.Stdio.Env[0].Value)
	}

	// http server: Url + Headers expansion.
	httpCfg := ServerConfig{
		Type:    "http",
		URL:     "${MCP_URL}/path",
		Headers: map[string]string{"X-Token": "${MCP_HDR}"},
	}
	hsrv, err := ToACP("h", httpCfg)
	if err != nil {
		t.Fatalf("ToACP http: %v", err)
	}
	if hsrv.Http.Url != "https://example.com/path" {
		t.Errorf("Url = %q, want https://example.com/path", hsrv.Http.Url)
	}
	if hsrv.Http.Headers[0].Value != "tok123" {
		t.Errorf("Header value = %q, want tok123", hsrv.Http.Headers[0].Value)
	}
}

// TestToACPEnvExpansionUnknownVar verifies unknown ${VAR} expands to empty.
func TestToACPEnvExpansionUnknownVar(t *testing.T) {
	cfg := ServerConfig{Command: "c", Args: []string{"${MCP_UNKNOWN}_suffix"}}
	srv, err := ToACP("u", cfg)
	if err != nil {
		t.Fatalf("ToACP: %v", err)
	}
	if srv.Stdio.Args[0] != "_suffix" {
		t.Errorf("Args[0] = %q, want _suffix (unknown var expands to empty)", srv.Stdio.Args[0])
	}
}

// TestToACPSliceCapabilityFiltering verifies ToACPSlice drops http/sse servers
// the agent doesn't support while always keeping stdio servers.
func TestToACPSliceCapabilityFiltering(t *testing.T) {
	t.Parallel()
	f := &File{
		Version: CurrentVersion,
		McpServers: map[string]ServerConfig{
			"local": {Command: "c"},
			"http1": {Type: "http", URL: "u"},
			"sse1":  {Type: "sse", URL: "u"},
			"off":   {Command: "c", Enabled: boolPtr(false)}, // disabled, dropped
		},
	}

	// Agent supports nothing extra: only stdio survives.
	got, err := ToACPSlice(f, acp.McpCapabilities{})
	if err != nil {
		t.Fatalf("ToACPSlice: %v", err)
	}
	if len(got) != 1 || got[0].Stdio == nil || got[0].Stdio.Name != "local" {
		t.Errorf("caps=none: got %+v, want only [local stdio]", got)
	}

	// Agent supports http+sse: all three enabled servers survive.
	got, err = ToACPSlice(f, acp.McpCapabilities{Http: true, Sse: true})
	if err != nil {
		t.Fatalf("ToACPSlice: %v", err)
	}
	if len(got) != 3 {
		t.Errorf("caps=http,sse: got %d servers, want 3", len(got))
	}
	// Order must be sorted by name: http1, local, sse1.
	names := make([]string, len(got))
	for i, s := range got {
		switch {
		case s.Stdio != nil:
			names[i] = s.Stdio.Name
		case s.Http != nil:
			names[i] = s.Http.Name
		case s.Sse != nil:
			names[i] = s.Sse.Name
		}
	}
	want := []string{"http1", "local", "sse1"}
	if !reflect.DeepEqual(names, want) {
		t.Errorf("order = %v, want %v", names, want)
	}
}

// TestToACPSliceBadType verifies ToACPSlice returns an error (not a partial
// list) when a server has an unknown type.
func TestToACPSliceBadType(t *testing.T) {
	t.Parallel()
	f := &File{
		Version: CurrentVersion,
		McpServers: map[string]ServerConfig{
			"bad": {Type: "ftp"},
		},
	}
	if _, err := ToACPSlice(f, acp.McpCapabilities{}); err == nil {
		t.Fatal("expected error for unknown type, got nil")
	}
}

// TestSaveNilFile verifies Save rejects a nil File rather than writing an empty
// doc, which would silently erase the user's config.
func TestSaveNilFile(t *testing.T) {
	t.Parallel()
	path := filepath.Join(t.TempDir(), "mcp.json")
	if err := Save(path, nil); err == nil {
		t.Fatal("Save(nil) expected error, got nil")
	}
}

// TestNewFileDefaults verifies NewFile returns a valid empty envelope.
func TestNewFileDefaults(t *testing.T) {
	t.Parallel()
	f := NewFile()
	if f.Version != CurrentVersion {
		t.Errorf("Version = %d, want %d", f.Version, CurrentVersion)
	}
	if f.McpServers == nil {
		t.Error("McpServers is nil, want empty map")
	}
}

// TestLoadParseError verifies Load returns an error (not an empty File) when
// the file exists but is not valid JSON.
func TestLoadParseError(t *testing.T) {
	t.Parallel()
	path := filepath.Join(t.TempDir(), "mcp.json")
	if err := os.WriteFile(path, []byte("{not json"), configFilePerm); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	if _, err := Load(path); err == nil {
		t.Fatal("Load of invalid JSON expected error, got nil")
	}
}

// TestSavePreservesFormatting verifies the on-disk JSON is indented and stable
// (keys in struct order, map keys sorted by encoding/json default) so diffs
// are readable and round-trips don't churn the file.
func TestSavePreservesFormatting(t *testing.T) {
	t.Parallel()
	path := filepath.Join(t.TempDir(), "mcp.json")
	f := NewFile()
	f.McpServers["a"] = ServerConfig{Command: "c"}
	if err := Save(path, f); err != nil {
		t.Fatalf("Save: %v", err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	// Must be valid indented JSON (contains a newline + two-space indent).
	if !strings.Contains(string(data), "\n  ") {
		t.Errorf("expected indented JSON, got:\n%s", string(data))
	}
	// Round-trip via json.Unmarshal to confirm it's valid.
	var roundTrip File
	if err := json.Unmarshal(data, &roundTrip); err != nil {
		t.Errorf("round-trip unmarshal: %v", err)
	}
}
