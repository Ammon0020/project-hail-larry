package mcp

import (
	"testing"
	"time"
)

// TestCheckHealth_StdioHealthy verifies that a stdio server with a real
// binary (e.g. "echo") reports "healthy".
func TestCheckHealth_StdioHealthy(t *testing.T) {
	enabled := true
	f := &File{
		Version: CurrentVersion,
		McpServers: map[string]ServerConfig{
			"test-echo": {
				Command: "echo",
				Enabled: &enabled,
			},
		},
	}

	results := CheckHealth(f, 1*time.Second)
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Name != "test-echo" {
		t.Errorf("expected name 'test-echo', got %q", results[0].Name)
	}
	if results[0].Status != "healthy" {
		t.Errorf("expected healthy, got %q (error: %s)", results[0].Status, results[0].Error)
	}
	if !results[0].Enabled {
		t.Error("expected enabled=true")
	}
}

// TestCheckHealth_StdioUnhealthy verifies that a stdio server with a
// nonexistent binary reports "unhealthy".
func TestCheckHealth_StdioUnhealthy(t *testing.T) {
	enabled := true
	f := &File{
		Version: CurrentVersion,
		McpServers: map[string]ServerConfig{
			"bad-server": {
				Command: "nonexistent-binary-xyz-12345",
				Enabled: &enabled,
			},
		},
	}

	results := CheckHealth(f, 1*time.Second)
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Status != "unhealthy" {
		t.Errorf("expected unhealthy, got %q", results[0].Status)
	}
	if results[0].Error == "" {
		t.Error("expected non-empty error message")
	}
}

// TestCheckHealth_Disabled verifies that disabled servers are reported as
// "disabled" without performing any check.
func TestCheckHealth_Disabled(t *testing.T) {
	disabled := false
	f := &File{
		Version: CurrentVersion,
		McpServers: map[string]ServerConfig{
			"disabled-server": {
				Command: "echo",
				Enabled: &disabled,
			},
		},
	}

	results := CheckHealth(f, 1*time.Second)
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Status != "disabled" {
		t.Errorf("expected disabled, got %q", results[0].Status)
	}
	if results[0].Enabled {
		t.Errorf("expected Enabled=false for disabled server, got true")
	}
}

// TestCheckHealth_NilFile returns nil when passed a nil File.
func TestCheckHealth_NilFile(t *testing.T) {
	results := CheckHealth(nil, 1*time.Second)
	if results != nil {
		t.Errorf("expected nil for nil file, got %v", results)
	}
}

// TestCheckHealth_EmptyServers returns nil when no servers are configured.
func TestCheckHealth_EmptyServers(t *testing.T) {
	f := &File{
		Version:    CurrentVersion,
		McpServers: map[string]ServerConfig{},
	}
	results := CheckHealth(f, 1*time.Second)
	if results != nil {
		t.Errorf("expected nil for empty servers, got %v", results)
	}
}

// TestCheckHealth_HttpUnreachable verifies that an http server with an
// unreachable host reports "unhealthy".
func TestCheckHealth_HttpUnreachable(t *testing.T) {
	enabled := true
	f := &File{
		Version: CurrentVersion,
		McpServers: map[string]ServerConfig{
			"http-bad": {
				Type:    "http",
				URL:     "http://192.0.2.1:12345", // RFC 5737 TEST-NET — guaranteed unreachable.
				Enabled: &enabled,
			},
		},
	}

	// Use a very short timeout so the test doesn't take forever.
	results := CheckHealth(f, 200*time.Millisecond)
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Status != "unhealthy" {
		t.Errorf("expected unhealthy for unreachable HTTP, got %q", results[0].Status)
	}
	if results[0].Error == "" {
		t.Error("expected non-empty error message")
	}
}

// TestCheckHealth_StdioNoCommand verifies that a stdio server with no command
// reports "unhealthy" with a clear error.
func TestCheckHealth_StdioNoCommand(t *testing.T) {
	enabled := true
	f := &File{
		Version: CurrentVersion,
		McpServers: map[string]ServerConfig{
			"empty-cmd": {
				Command: "",
				Enabled: &enabled,
			},
		},
	}

	results := CheckHealth(f, 1*time.Second)
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Status != "unhealthy" {
		t.Errorf("expected unhealthy, got %q", results[0].Status)
	}
	if results[0].Error != errNoCommand {
		t.Errorf("unexpected error: %q", results[0].Error)
	}
}

// TestCheckHealth_UrlWithoutTypeInferredAsHttp verifies that a server with a
// `url` but no `type` field (and no `command`) is inferred as HTTP and checked
// via TCP dial, not treated as a stdio server with an empty command. This is
// the context7 shape: {"url": "https://...", "headers": {...}} with no type.
func TestCheckHealth_UrlWithoutTypeInferredAsHttp(t *testing.T) {
	enabled := true
	f := &File{
		Version: CurrentVersion,
		McpServers: map[string]ServerConfig{
			"context7-like": {
				URL:     "http://192.0.2.1:12345", // RFC 5737 TEST-NET — unreachable.
				Headers: map[string]string{"API_KEY": "tok"},
				Enabled: &enabled,
			},
		},
	}

	results := CheckHealth(f, 200*time.Millisecond)
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	// Must be unhealthy due to connection failure — NOT "no command configured"
	// (which would indicate it was misclassified as stdio).
	if results[0].Status != "unhealthy" {
		t.Errorf("expected unhealthy (connection failed), got %q", results[0].Status)
	}
	if results[0].Error == "no command configured" {
		t.Error("server with URL was misclassified as stdio — expected HTTP check")
	}
}

// TestCheckHealth_DeterministicOrder verifies the output is sorted by server
// name so results are deterministic regardless of map iteration order.
func TestCheckHealth_DeterministicOrder(t *testing.T) {
	enabled := true
	f := &File{
		Version: CurrentVersion,
		McpServers: map[string]ServerConfig{
			"zebra":  {Command: "echo", Enabled: &enabled},
			"alpha":  {Command: "echo", Enabled: &enabled},
			"middle": {Command: "echo", Enabled: &enabled},
		},
	}

	results := CheckHealth(f, 1*time.Second)
	if len(results) != 3 {
		t.Fatalf("expected 3 results, got %d", len(results))
	}
	if results[0].Name != "alpha" || results[1].Name != "middle" || results[2].Name != "zebra" {
		t.Errorf("expected sorted order [alpha, middle, zebra], got [%s, %s, %s]",
			results[0].Name, results[1].Name, results[2].Name)
	}
}
