package acp

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
)

// buildMockAgent builds the mockagent binary and returns its path.
// It uses a shared temp dir so multiple tests don't rebuild.
var (
	mockAgentOnce sync.Once
	mockAgentPath string
	mockAgentErr  error
)

func buildMockAgent(t *testing.T) string {
	t.Helper()
	mockAgentOnce.Do(func() {
		// Find the project root (where go.mod is).
		_, filename, _, _ := runtime.Caller(0)
		projectRoot := filepath.Join(filepath.Dir(filename), "..", "..")

		// Use os.MkdirTemp instead of t.TempDir so the binary persists
		// across multiple tests — t.TempDir is cleaned up after each test.
		tmpDir, err := os.MkdirTemp("", "mockagent-*")
		if err != nil {
			mockAgentErr = fmt.Errorf("create temp dir: %w", err)
			return
		}
		binaryName := "mockagent"
		if runtime.GOOS == "windows" {
			binaryName = "mockagent.exe"
		}
		path := filepath.Join(tmpDir, binaryName)

		cmd := exec.Command("go", "build", "-o", path, "./cmd/mockagent")
		cmd.Dir = projectRoot
		cmd.Stderr = os.Stderr
		if err := cmd.Run(); err != nil {
			mockAgentErr = fmt.Errorf("build mockagent: %w", err)
			return
		}
		mockAgentPath = path
	})
	if mockAgentErr != nil {
		t.Fatalf("failed to build mockagent: %v", mockAgentErr)
	}
	return mockAgentPath
}

// TestMockAgentFullFlow tests the complete ACP pipeline using the mock agent:
// 1. Register agent
// 2. Create session (spawns mockagent process)
// 3. Send prompt
// 4. Receive streaming events (PromptSubmitted, ResponseStarted, StreamUpdate, ToolStarted, ToolCompleted)
// 5. Verify event sequence and content
func TestMockAgentFullFlow(t *testing.T) {
	agentPath := buildMockAgent(t)

	client := NewClient(nil, nil)
	ctx := context.Background()

	client.RegisterAgent(AgentInfo{
		ID:      "mock",
		Name:    "Mock Agent",
		Command: agentPath,
		Models: []AgentModel{
			{ID: "mock-model", Name: "Mock Model"},
		},
	})

	cb := &mockCallbacks{}
	client.SetCallbacks(cb)

	// Create session — this spawns the mockagent process and does ACP handshake.
	session, err := client.CreateSession(ctx, "mock", "mock-model", ".")
	if err != nil {
		t.Fatalf("create session: %v", err)
	}
	t.Logf("created session: %s", session.ID)

	// Send prompt — this runs in a goroutine, events arrive via callbacks.
	err = client.SendPrompt(ctx, session.ID, "Hello, mock agent!")
	if err != nil {
		t.Fatalf("send prompt: %v", err)
	}

	// Wait for events to arrive. The mock agent streams text + runs shell commands.
	// We expect: PromptSubmitted, ResponseStarted, StreamUpdate(s), ToolStarted,
	// ToolCompleted, StreamUpdate(s), ToolStarted, ToolCompleted, StreamUpdate(s),
	// final StreamUpdate (streaming=false).
	deadline := time.After(15 * time.Second)
	for {
		select {
		case <-deadline:
			t.Fatalf("timed out waiting for events; got %d events: %v", len(cb.events), eventTypes(cb.events))
		case <-time.After(50 * time.Millisecond):
		}

		cb.mu.Lock()
		count := len(cb.events)
		cb.mu.Unlock()

		// We expect at least: PromptSubmitted + ResponseStarted + several StreamUpdates
		// + 2x ToolStarted + 2x ToolCompleted + final StreamUpdate.
		// The mock agent streams word-by-word, so there will be many events.
		if count >= 5 {
			// Check if we've received the final streaming=false event.
			cb.mu.Lock()
			done := false
			for _, e := range cb.events {
				if e.Type == interfaces.EventStreamUpdate && !e.Streaming {
					done = true
					break
				}
			}
			cb.mu.Unlock()
			if done {
				break
			}
		}
	}

	cb.mu.Lock()
	defer cb.mu.Unlock()
	events := cb.events
	t.Logf("received %d events", len(events))

	// Verify event sequence.
	var (
		hasPromptSubmitted bool
		hasResponseStarted bool
		hasStreamUpdate    bool
		hasToolStarted     bool
		hasToolCompleted   bool
		streamContent      strings.Builder
	)
	for _, e := range events {
		switch e.Type {
		case interfaces.EventPromptSubmitted:
			hasPromptSubmitted = true
			if e.Content != "Hello, mock agent!" {
				t.Errorf("expected prompt content 'Hello, mock agent!', got %q", e.Content)
			}
			if e.Role != "user" {
				t.Errorf("expected role 'user', got %q", e.Role)
			}
		case interfaces.EventResponseStarted:
			hasResponseStarted = true
		case interfaces.EventStreamUpdate:
			hasStreamUpdate = true
			streamContent.WriteString(e.Content)
		case interfaces.EventToolStarted:
			hasToolStarted = true
		case interfaces.EventToolCompleted:
			hasToolCompleted = true
		case interfaces.EventAgentExited:
			t.Errorf("unexpected AgentExited event: %s", e.Summary)
		}
	}

	if !hasPromptSubmitted {
		t.Error("missing PromptSubmitted event")
	}
	if !hasResponseStarted {
		t.Error("missing ResponseStarted event")
	}
	if !hasStreamUpdate {
		t.Error("missing StreamUpdate event")
	}
	if !hasToolStarted {
		t.Error("missing ToolStarted event")
	}
	if !hasToolCompleted {
		t.Error("missing ToolCompleted event")
	}

	// Verify the streamed content contains expected text.
	combined := streamContent.String()
	if !strings.Contains(combined, "Hello, mock agent!") {
		t.Errorf("streamed content does not contain user message echo; got: %s", combined)
	}
	if !strings.Contains(combined, "All done") {
		t.Errorf("streamed content does not contain 'All done'; got: %s", combined)
	}
	t.Logf("streamed content (%d chars): %s", len(combined), combined)

	// Clean up.
	_ = client.CloseSession(ctx, session.ID)
}

// TestMockAgentMultiplePrompts verifies that multiple prompts can be sent
// to the same session sequentially.
func TestMockAgentMultiplePrompts(t *testing.T) {
	agentPath := buildMockAgent(t)

	client := NewClient(nil, nil)
	ctx := context.Background()

	client.RegisterAgent(AgentInfo{
		ID:      "mock",
		Name:    "Mock Agent",
		Command: agentPath,
		Models:  []AgentModel{{ID: "mock-model", Name: "Mock Model"}},
	})

	cb := &mockCallbacks{}
	client.SetCallbacks(cb)

	session, err := client.CreateSession(ctx, "mock", "mock-model", ".")
	if err != nil {
		t.Fatalf("create session: %v", err)
	}

	// Send first prompt.
	if err := client.SendPrompt(ctx, session.ID, "First message"); err != nil {
		t.Fatalf("send prompt 1: %v", err)
	}
	waitForPromptComplete(t, cb, 1)

	// Send second prompt.
	if err := client.SendPrompt(ctx, session.ID, "Second message"); err != nil {
		t.Fatalf("send prompt 2: %v", err)
	}
	waitForPromptComplete(t, cb, 2)

	// Verify we got events for both prompts.
	cb.mu.Lock()
	defer cb.mu.Unlock()
	promptCount := 0
	for _, e := range cb.events {
		if e.Type == interfaces.EventPromptSubmitted {
			promptCount++
		}
	}
	if promptCount < 2 {
		t.Errorf("expected at least 2 PromptSubmitted events, got %d", promptCount)
	}

	_ = client.CloseSession(ctx, session.ID)
}

// TestRealAgentDevstral tests against a real devstral-small agent.
// Skipped by default — set ACP_TEST_REAL=1 to run.
func TestRealAgentDevstral(t *testing.T) {
	if os.Getenv("ACP_TEST_REAL") == "" {
		t.Skip("Set ACP_TEST_REAL=1 to run real agent tests")
	}

	client := NewClient(nil, nil)
	ctx := context.Background()

	// Try to find devstral-small via autodetect.
	agents := Autodetect()
	var devstralCmd string
	for _, a := range agents {
		if strings.Contains(strings.ToLower(a.Name), "devstral") || strings.Contains(strings.ToLower(a.ID), "devstral") {
			devstralCmd = a.Command
			client.RegisterAgent(AgentInfo{
				ID:      a.ID,
				Name:    a.Name,
				Command: a.Command,
				Args:    a.Args,
				Models:  a.Models,
			})
			break
		}
	}
	if devstralCmd == "" {
		t.Skip("devstral-small not found in autodetect")
	}

	cb := &mockCallbacks{}
	client.SetCallbacks(cb)

	// Find a model.
	agentList, _ := client.ListAgents(ctx)
	if len(agentList) == 0 || len(agentList[0].Models) == 0 {
		t.Skip("no models available for devstral")
	}
	modelID := agentList[0].Models[0].ID

	session, err := client.CreateSession(ctx, agentList[0].ID, modelID, ".")
	if err != nil {
		t.Fatalf("create session: %v", err)
	}
	t.Logf("created session with devstral: %s", session.ID)

	if err := client.SendPrompt(ctx, session.ID, "Say hello in one sentence."); err != nil {
		t.Fatalf("send prompt: %v", err)
	}

	// Wait up to 60 seconds for real LLM response.
	deadline := time.After(60 * time.Second)
	for {
		select {
		case <-deadline:
			t.Fatalf("timed out waiting for devstral response")
		case <-time.After(200 * time.Millisecond):
		}
		cb.mu.Lock()
		done := false
		for _, e := range cb.events {
			if e.Type == interfaces.EventStreamUpdate && !e.Streaming {
				done = true
				break
			}
			if e.Type == interfaces.EventAgentExited {
				t.Fatalf("agent exited: %s", e.Summary)
			}
		}
		cb.mu.Unlock()
		if done {
			break
		}
	}

	cb.mu.Lock()
	defer cb.mu.Unlock()
	var content strings.Builder
	for _, e := range cb.events {
		if e.Type == interfaces.EventStreamUpdate {
			content.WriteString(e.Content)
		}
	}
	t.Logf("devstral response: %s", content.String())
	if content.Len() == 0 {
		t.Error("no streamed content received from devstral")
	}

	_ = client.CloseSession(ctx, session.ID)
}

// TestRealAgentCodex tests against a real codex agent (GPT model).
// Skipped by default — set ACP_TEST_REAL=1 to run.
func TestRealAgentCodex(t *testing.T) {
	if os.Getenv("ACP_TEST_REAL") == "" {
		t.Skip("Set ACP_TEST_REAL=1 to run real agent tests")
	}

	client := NewClient(nil, nil)
	ctx := context.Background()

	agents := Autodetect()
	var codexFound bool
	for _, a := range agents {
		if strings.Contains(strings.ToLower(a.Name), "codex") || strings.Contains(strings.ToLower(a.ID), "codex") {
			client.RegisterAgent(AgentInfo{
				ID:      a.ID,
				Name:    a.Name,
				Command: a.Command,
				Args:    a.Args,
				Models:  a.Models,
			})
			codexFound = true
			break
		}
	}
	if !codexFound {
		t.Skip("codex not found in autodetect")
	}

	cb := &mockCallbacks{}
	client.SetCallbacks(cb)

	agentList, _ := client.ListAgents(ctx)
	if len(agentList) == 0 || len(agentList[0].Models) == 0 {
		t.Skip("no models available for codex")
	}
	modelID := agentList[0].Models[0].ID

	session, err := client.CreateSession(ctx, agentList[0].ID, modelID, ".")
	if err != nil {
		t.Fatalf("create session: %v", err)
	}
	t.Logf("created session with codex: %s", session.ID)

	if err := client.SendPrompt(ctx, session.ID, "Say hello in one sentence."); err != nil {
		t.Fatalf("send prompt: %v", err)
	}

	deadline := time.After(60 * time.Second)
	for {
		select {
		case <-deadline:
			t.Fatalf("timed out waiting for codex response")
		case <-time.After(200 * time.Millisecond):
		}
		cb.mu.Lock()
		done := false
		for _, e := range cb.events {
			if e.Type == interfaces.EventStreamUpdate && !e.Streaming {
				done = true
				break
			}
			if e.Type == interfaces.EventAgentExited {
				t.Fatalf("agent exited: %s", e.Summary)
			}
		}
		cb.mu.Unlock()
		if done {
			break
		}
	}

	cb.mu.Lock()
	defer cb.mu.Unlock()
	var content strings.Builder
	for _, e := range cb.events {
		if e.Type == interfaces.EventStreamUpdate {
			content.WriteString(e.Content)
		}
	}
	t.Logf("codex response: %s", content.String())
	if content.Len() == 0 {
		t.Error("no streamed content received from codex")
	}

	_ = client.CloseSession(ctx, session.ID)
}

// waitForPromptComplete waits until the Nth prompt's response is complete
// (indicated by a final StreamUpdate with streaming=false after the Nth
// PromptSubmitted event).
func waitForPromptComplete(t *testing.T, cb *mockCallbacks, promptNum int) {
	t.Helper()
	deadline := time.After(15 * time.Second)
	for {
		select {
		case <-deadline:
			cb.mu.Lock()
			count := len(cb.events)
			cb.mu.Unlock()
			t.Fatalf("timed out waiting for prompt %d to complete; got %d events", promptNum, count)
		case <-time.After(50 * time.Millisecond):
		}
		cb.mu.Lock()
		promptCount := 0
		done := false
		for _, e := range cb.events {
			if e.Type == interfaces.EventPromptSubmitted {
				promptCount++
			}
			// The final streaming=false event after the Nth prompt means we're done.
			if e.Type == interfaces.EventStreamUpdate && !e.Streaming && promptCount >= promptNum {
				done = true
			}
		}
		cb.mu.Unlock()
		if done {
			break
		}
	}
}

// eventTypes returns a slice of event type strings for debugging.
func eventTypes(events []interfaces.Event) []string {
	types := make([]string, len(events))
	for i, e := range events {
		types[i] = string(e.Type)
	}
	return types
}
