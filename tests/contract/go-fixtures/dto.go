// Package main: shared DTO serialization fixtures.
//
// The Rust port must reproduce the exact JSON shapes the Go daemon emits for
// shared types. This capturer instantiates representative values of each
// shared DTO and marshals them to golden/dto/<type>.json. The values are
// constructed to exercise optional/omitempty fields so the Rust side can see
// which fields are dropped when empty.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/adama/local-agent/internal/acp"
	"github.com/adama/local-agent/internal/config"
	"github.com/adama/local-agent/internal/interfaces"
	"github.com/adama/local-agent/internal/pairing"
)

// captureDTO marshals representative values of every shared DTO and writes
// golden/dto/<type>.json. All secrets and absolute paths are redacted.
func captureDTO(h *harness, goldenDir string) error {
	outDir := filepath.Join(goldenDir, "dto")
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		return fmt.Errorf("mkdir %s: %w", outDir, err)
	}

	// A representative timestamp used across DTOs. A fixed instant keeps the
	// fixtures byte-stable for exact comparison of time fields.
	ts := time.Date(2026, 7, 13, 12, 0, 0, 0, time.UTC)

	type dtoEntry struct {
		name string
		val  any
	}

	entries := []dtoEntry{
		{"config_default", defaultConfigDTO()},
		{"event_full", fullEventDTO(ts)},
		{"event_minimal", minimalEventDTO(ts)},
		{"workspace_info", interfaces.WorkspaceInfo{ID: "<REDACTED_WORKSPACE_ID>", Path: "<REDACTED_PATH>", Name: "seed-workspace"}},
		{"session_info", interfaces.SessionInfo{ID: "fixture-session", Name: "Fixture Session", Status: "ready", AgentID: "fixture-agent", ModelID: "fixture-model", Workspace: "<REDACTED_PATH>", CreatedAt: ts, UpdatedAt: ts}},
		{"agent_info", acp.AgentInfo{ID: "fixture-agent", Name: "Fixture Agent", Command: "fixture-agent-binary", Args: []string{"--acp"}, Models: []acp.AgentModel{{ID: "fixture-model", Name: "Fixture Model"}}, Warning: "Executable not found in PATH"}},
		{"agent_info_empty_optional", acp.AgentInfo{ID: "bare-agent", Name: "Bare", Command: "bare", Models: nil}},
		{"pairing_session", pairing.PairingSession{ID: "fixture-session-id", Token: "<REDACTED_TOKEN>", Passcode: "<REDACTED_PASSCODE>", URL: "http://localhost:7337?token=<REDACTED_TOKEN>", QRPath: "<REDACTED_PATH>/qr.png", CreatedAt: ts, ExpiresAt: ts.Add(5 * time.Minute), Used: false}},
		{"device_credential", pairing.DeviceCredential{ID: "<REDACTED_DEVICE_ID>", Name: "fixture-device", Secret: "<REDACTED_TOKEN>", PairedAt: ts}},
		{"device_info", pairing.DeviceInfo{ID: "<REDACTED_DEVICE_ID>", Name: "fixture-device", PairedAt: ts, LastSeen: ts}},
		{"pending_action_info", pairing.PendingActionInfo{ID: "fixture-action", Type: pairing.PendingActionTypeRevocation, DeviceID: "<REDACTED_DEVICE_ID>", DeviceName: "fixture-device", RequestedBy: "<REDACTED_DEVICE_ID>", RequestedAt: ts, ExecuteAt: ts.Add(5 * time.Minute)}},
		{"file_node_folder", interfaces.FileNode{Name: "src", Type: interfaces.FileNodeTypeFolder, Path: "src", Children: []interfaces.FileNode{{Name: "greet.txt", Type: interfaces.FileNodeTypeFile, Path: "src/greet.txt"}}}},
		{"file_node_file", interfaces.FileNode{Name: "README.md", Type: interfaces.FileNodeTypeFile, Path: "README.md"}},
		{"provider_info", interfaces.ProviderInfo{ID: "main", Required: true, Supported: []string{"anthropic", "openai"}, Current: &interfaces.ProviderCurrentConfig{APIType: "anthropic", BaseURL: "https://api.anthropic.com"}}},
		{"provider_info_disabled", interfaces.ProviderInfo{ID: "openai", Required: false, Supported: []string{"openai"}}},
		{"attachment", interfaces.Attachment{ID: "fixture-upload", Name: "screenshot.png", MimeType: "image/png", Path: "<REDACTED_PATH>/uploads/screenshot.png"}},
	}

	for _, e := range entries {
		data, err := json.MarshalIndent(e.val, "", "  ")
		if err != nil {
			return fmt.Errorf("marshal dto %s: %w", e.name, err)
		}
		// Redact in case any constructed value accidentally embeds a real
		// path/secret (defensive; the values above are already placeholders).
		redacted := h.redactor.String(string(data))
		path := filepath.Join(outDir, e.name+".json")
		if err := os.WriteFile(path, []byte(redacted+"\n"), 0o644); err != nil {
			return fmt.Errorf("write %s: %w", e.name, err)
		}
	}
	return nil
}

// defaultConfigDTO returns a config.Config with default-shaped values and
// redacted paths, exercising the omitempty fields the Rust port must mirror.
func defaultConfigDTO() config.Config {
	return config.Config{
		Port:                            7337,
		Host:                            "0.0.0.0",
		DataDir:                         "<REDACTED_PATH>",
		DBPath:                          "<REDACTED_PATH>/local-agent.db",
		Workspaces:                      []string{"<REDACTED_PATH>/seed-workspace"},
		Agents:                          []acp.AgentInfo{{ID: "fixture-agent", Name: "Fixture Agent", Command: "fixture-agent-binary", Models: []acp.AgentModel{{ID: "fixture-model", Name: "Fixture Model"}}}},
		TLSEnabled:                      true,
		TLSCertDir:                      "<REDACTED_PATH>/tls",
		HTTPSPort:                       0, // exercises omitempty
		PairingTTLSeconds:               300,
		CredentialInactivityTTLSeconds:  2592000,
		AllowRemoteWorkspaceRegistration: false,
		RevocationGracePeriodSeconds:    300,
	}
}

// fullEventDTO returns an Event populated with every field the Rust port must
// serialize, exercising omitempty behavior across the union of event types.
func fullEventDTO(ts time.Time) interfaces.Event {
	exitCode := 7
	return interfaces.Event{
		ID:         42,
		Type:       interfaces.EventToolCompleted,
		SessionID:  "fixture-session",
		Timestamp:  ts,
		Role:       "agent",
		Content:    "fixture content",
		Streaming:  false,
		Tool:       "read_text_file",
		Target:     "src/greet.txt",
		Summary:    "fixture summary",
		Command:    "ls -la",
		Cwd:        "<REDACTED_PATH>",
		Options:    []string{"allow_always", "allow_session", "deny"},
		RequestID:  "fixture-request",
		ToolKind:   "read",
		ToolCallID: "fixture-tool-call",
		Thought:    true,
		ExitCode:   &exitCode,
		StopReason: "end_turn",
		WorkspaceID: "<REDACTED_WORKSPACE_ID>",
		Attachments: []interfaces.Attachment{{ID: "fixture-upload", Name: "screenshot.png", MimeType: "image/png", Path: "<REDACTED_PATH>/uploads/screenshot.png"}},
		ExecuteAt:   ts.Add(5 * time.Minute),
		DeviceName:  "fixture-device",
	}
}

// minimalEventDTO returns an Event with only the required fields set, so the
// Rust port can verify omitempty drops the rest.
func minimalEventDTO(ts time.Time) interfaces.Event {
	return interfaces.Event{
		ID:        1,
		Type:      interfaces.EventPromptSubmitted,
		SessionID: "fixture-session",
		Timestamp: ts,
		Role:      "user",
		Content:   "hello",
	}
}
