package acp

import (
	"context"
	"testing"
)

// TestProfileMiddleware_CodeMode verifies that the default "Code" profile
// injects the Code instructions text.
func TestProfileMiddleware_CodeMode(t *testing.T) {
	mw := NewProfileMiddleware(nil)
	mw.SetProfile("s1", "Code")

	// Text path (embeddedContext = false).
	pc := &PromptContext{SessionID: "s1", EmbeddedContext: false}
	action, body := mw.BeforePrompt(context.Background(), pc)
	if action != ActionInject {
		t.Fatalf("expected ActionInject, got %v", action)
	}
	if body == "" {
		t.Fatal("expected non-empty injection for Code mode")
	}
	if !containsAll(body, "Active Profile: Code", "CODE mode") {
		t.Errorf("unexpected body: %q", body)
	}

	// No resources when embeddedContext is false.
	res := mw.BeforePromptResources(context.Background(), pc)
	if len(res) != 0 {
		t.Errorf("expected no resources without embeddedContext, got %d", len(res))
	}
}

// TestProfileMiddleware_AskMode verifies Ask mode injects "do NOT modify" text.
func TestProfileMiddleware_AskMode(t *testing.T) {
	mw := NewProfileMiddleware(nil)
	mw.SetProfile("s1", "Ask")

	pc := &PromptContext{SessionID: "s1", EmbeddedContext: false}
	action, body := mw.BeforePrompt(context.Background(), pc)
	if action != ActionInject {
		t.Fatalf("expected ActionInject, got %v", action)
	}
	if !containsAll(body, "Active Profile: Ask", "ASK mode", "Do NOT modify") {
		t.Errorf("unexpected Ask body: %q", body)
	}
}

// TestProfileMiddleware_PlanMode verifies Plan mode injects "step-by-step" text.
func TestProfileMiddleware_PlanMode(t *testing.T) {
	mw := NewProfileMiddleware(nil)
	mw.SetProfile("s1", "Plan")

	pc := &PromptContext{SessionID: "s1", EmbeddedContext: false}
	action, body := mw.BeforePrompt(context.Background(), pc)
	if action != ActionInject {
		t.Fatalf("expected ActionInject, got %v", action)
	}
	if !containsAll(body, "Active Profile: Plan", "PLAN mode", "step-by-step") {
		t.Errorf("unexpected Plan body: %q", body)
	}
}

// TestProfileMiddleware_EmbeddedContextUsesResources verifies that when
// EmbeddedContext is true, the middleware emits a resource block instead of
// text injection.
func TestProfileMiddleware_EmbeddedContextUsesResources(t *testing.T) {
	mw := NewProfileMiddleware(nil)
	mw.SetProfile("s1", "Ask")

	pc := &PromptContext{SessionID: "s1", EmbeddedContext: true}

	// Text path should return ActionContinue (no text injection).
	action, _ := mw.BeforePrompt(context.Background(), pc)
	if action != ActionContinue {
		t.Fatalf("expected ActionContinue with embeddedContext, got %v", action)
	}

	// Resources path should return a resource block.
	res := mw.BeforePromptResources(context.Background(), pc)
	if len(res) != 1 {
		t.Fatalf("expected 1 resource, got %d", len(res))
	}
	if res[0].URI != profileContextURI {
		t.Errorf("expected URI %q, got %q", profileContextURI, res[0].URI)
	}
	if res[0].MimeType != contextMimeType {
		t.Errorf("expected mimeType %q, got %q", contextMimeType, res[0].MimeType)
	}
	if !containsAll(res[0].Text, "Active Profile: Ask", "ASK mode") {
		t.Errorf("unexpected resource text: %q", res[0].Text)
	}
}

// TestProfileMiddleware_DefaultsToCode verifies that when no profile is set
// for a session, the middleware defaults to "Code".
func TestProfileMiddleware_DefaultsToCode(t *testing.T) {
	mw := NewProfileMiddleware(nil)
	// No SetProfile call — should default to Code.

	pc := &PromptContext{SessionID: "s1", EmbeddedContext: false}
	action, body := mw.BeforePrompt(context.Background(), pc)
	if action != ActionInject {
		t.Fatalf("expected ActionInject, got %v", action)
	}
	if !containsAll(body, "Active Profile: Code", "CODE mode") {
		t.Errorf("expected Code default, got: %q", body)
	}
}

// TestProfileMiddleware_ClearProfile verifies ClearProfile removes the stored
// profile, reverting to the "Code" default.
func TestProfileMiddleware_ClearProfile(t *testing.T) {
	mw := NewProfileMiddleware(nil)
	mw.SetProfile("s1", "Plan")
	mw.ClearProfile("s1")

	pc := &PromptContext{SessionID: "s1", EmbeddedContext: false}
	_, body := mw.BeforePrompt(context.Background(), pc)
	if !containsAll(body, "Active Profile: Code") {
		t.Errorf("expected Code after clear, got: %q", body)
	}
}

// containsAll checks if s contains all the given substrings.
func containsAll(s string, subs ...string) bool {
	for _, sub := range subs {
		if !contains(s, sub) {
			return false
		}
	}
	return true
}

// contains checks if s contains sub.
func contains(s, sub string) bool {
	return len(s) >= len(sub) && searchString(s, sub)
}

func searchString(s, sub string) bool {
	for i := 0; i <= len(s)-len(sub); i++ {
		if s[i:i+len(sub)] == sub {
			return true
		}
	}
	return false
}
