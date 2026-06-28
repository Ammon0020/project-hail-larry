package acp

import (
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

func TestDefaultSystemMessages(t *testing.T) {
	sm := DefaultSystemMessages()
	if sm.WorkspaceContextHeader != "## Workspace Context" {
		t.Errorf("WorkspaceContextHeader = %q, want %q", sm.WorkspaceContextHeader, "## Workspace Context")
	}
	if sm.MaxContextBytes != 8*1024 {
		t.Errorf("MaxContextBytes = %d, want %d", sm.MaxContextBytes, 8*1024)
	}
	if sm.MaxContextFiles != 200 {
		t.Errorf("MaxContextFiles = %d, want %d", sm.MaxContextFiles, 200)
	}
	if sm.MaxFileTreeDepth != 3 {
		t.Errorf("MaxFileTreeDepth = %d, want %d", sm.MaxFileTreeDepth, 3)
	}
	if sm.MaxOpenFiles != 20 {
		t.Errorf("MaxOpenFiles = %d, want %d", sm.MaxOpenFiles, 20)
	}
	if sm.MaxRecentEdits != 10 {
		t.Errorf("MaxRecentEdits = %d, want %d", sm.MaxRecentEdits, 10)
	}
}

func TestLoadSystemMessages_ValidJSON(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "system-messages.json")
	custom := `{
		"workspaceContextHeader": "## Custom Context",
		"filesHeader": "## Files ({count} of depth {depth})",
		"maxContextFiles": 42,
		"maxOpenFiles": 5
	}`
	if err := os.WriteFile(path, []byte(custom), 0o644); err != nil {
		t.Fatalf("write config: %v", err)
	}

	sm, err := LoadSystemMessages(path)
	if err != nil {
		t.Fatalf("LoadSystemMessages: %v", err)
	}
	if sm.WorkspaceContextHeader != "## Custom Context" {
		t.Errorf("WorkspaceContextHeader = %q, want %q", sm.WorkspaceContextHeader, "## Custom Context")
	}
	if sm.MaxContextFiles != 42 {
		t.Errorf("MaxContextFiles = %d, want 42", sm.MaxContextFiles)
	}
	if sm.MaxOpenFiles != 5 {
		t.Errorf("MaxOpenFiles = %d, want 5", sm.MaxOpenFiles)
	}
	// Unspecified fields fall back to defaults.
	if sm.GitHeader != "## Git" {
		t.Errorf("GitHeader = %q, want %q (default)", sm.GitHeader, "## Git")
	}
	if sm.MaxContextBytes != 8*1024 {
		t.Errorf("MaxContextBytes = %d, want %d (default)", sm.MaxContextBytes, 8*1024)
	}
}

func TestLoadSystemMessages_MissingFileReturnsDefaults(t *testing.T) {
	sm, err := LoadSystemMessages(filepath.Join(t.TempDir(), "nope.json"))
	if err == nil {
		t.Error("expected error for missing file, got nil")
	}
	if sm == nil {
		t.Fatal("expected non-nil SystemMessages even on missing file")
	}
	if sm.WorkspaceContextHeader != "## Workspace Context" {
		t.Errorf("expected default header on missing file, got %q", sm.WorkspaceContextHeader)
	}
}

func TestLoadSystemMessages_InvalidJSONReturnsDefaults(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "bad.json")
	if err := os.WriteFile(path, []byte("{not valid json"), 0o644); err != nil {
		t.Fatalf("write config: %v", err)
	}
	sm, err := LoadSystemMessages(path)
	if err == nil {
		t.Error("expected error for invalid JSON, got nil")
	}
	if sm == nil {
		t.Fatal("expected non-nil SystemMessages even on invalid JSON")
	}
	if sm.MaxContextFiles != 200 {
		t.Errorf("expected default MaxContextFiles on invalid JSON, got %d", sm.MaxContextFiles)
	}
}

func TestSystemMessages_Render(t *testing.T) {
	sm := DefaultSystemMessages()
	got := sm.Render(sm.FilesHeader, map[string]string{
		"count": "42",
		"depth": "3",
	})
	want := "## Files (first 42, depth ≤ 3)"
	if got != want {
		t.Errorf("Render = %q, want %q", got, want)
	}
	// Unknown placeholders are left intact.
	got2 := sm.Render("hello {unknown} {count}", map[string]string{"count": "7"})
	want2 := "hello {unknown} 7"
	if got2 != want2 {
		t.Errorf("Render unknown = %q, want %q", got2, want2)
	}
	// Nil vars returns header unchanged.
	got3 := sm.Render(sm.GitHeader, nil)
	if got3 != sm.GitHeader {
		t.Errorf("Render nil vars = %q, want %q", got3, sm.GitHeader)
	}
}

func TestOpenFilesTracker(t *testing.T) {
	tr := NewOpenFilesTracker()
	if len(tr.OpenFiles()) != 0 {
		t.Errorf("expected empty open files, got %v", tr.OpenFiles())
	}
	tr.SetOpenFiles([]string{"a.go", "b.go"})
	tr.SetRecentEdits([]string{"c.go"})
	of := tr.OpenFiles()
	if len(of) != 2 || of[0] != "a.go" || of[1] != "b.go" {
		t.Errorf("OpenFiles = %v, want [a.go b.go]", of)
	}
	re := tr.RecentEdits()
	if len(re) != 1 || re[0] != "c.go" {
		t.Errorf("RecentEdits = %v, want [c.go]", re)
	}
	// SetOpenFiles replaces, not appends.
	tr.SetOpenFiles([]string{"d.go"})
	of = tr.OpenFiles()
	if len(of) != 1 || of[0] != "d.go" {
		t.Errorf("OpenFiles after replace = %v, want [d.go]", of)
	}
	// Returned slices are copies (mutating them must not affect the tracker).
	of[0] = "mutated"
	if tr.OpenFiles()[0] != "d.go" {
		t.Errorf("tracker mutated by caller: %v", tr.OpenFiles())
	}
}

// stubProvider is a test OpenFilesProvider returning fixed slices.
type stubProvider struct {
	openFiles   []string
	recentEdits []string
}

func (s *stubProvider) OpenFiles() []string   { return s.openFiles }
func (s *stubProvider) RecentEdits() []string { return s.recentEdits }

func TestTimeMiddleware_InjectsEveryPrompt(t *testing.T) {
	// Stub the clock so the test is deterministic.
	orig := nowFunc
	fixed := time.Date(2026, 6, 27, 15, 4, 5, 0, time.UTC)
	nowFunc = func() time.Time { return fixed }
	defer func() { nowFunc = orig }()

	mw := NewTimeMiddleware(nil)
	pc := &PromptContext{SessionID: "s1", PromptCount: 0}
	// First prompt.
	action1, msg1 := mw.BeforePrompt(nil, pc)
	if action1 != ActionInject {
		t.Fatalf("first prompt: expected ActionInject, got %v", action1)
	}
	if !strings.Contains(msg1, "## Current Time") {
		t.Errorf("first prompt: expected time header, got %q", msg1)
	}
	if !strings.Contains(msg1, fixed.Format(time.RFC3339)) {
		t.Errorf("first prompt: expected ISO time %q, got %q", fixed.Format(time.RFC3339), msg1)
	}
	// Second prompt also injects (time is per-prompt).
	pc.PromptCount = 1
	action2, msg2 := mw.BeforePrompt(nil, pc)
	if action2 != ActionInject {
		t.Fatalf("second prompt: expected ActionInject, got %v", action2)
	}
	if msg2 != msg1 {
		t.Errorf("second prompt: expected same injection, got %q vs %q", msg2, msg1)
	}
}

func TestOpenFilesMiddleware_SkipsWhenEmpty(t *testing.T) {
	mw := NewOpenFilesMiddleware(&stubProvider{}, nil)
	pc := &PromptContext{SessionID: "s1", PromptCount: 0}
	action, msg := mw.BeforePrompt(nil, pc)
	if action != ActionContinue {
		t.Errorf("expected ActionContinue when no open files, got %v (%q)", action, msg)
	}
	if msg != "" {
		t.Errorf("expected empty message, got %q", msg)
	}
}

func TestOpenFilesMiddleware_InjectsPaths(t *testing.T) {
	prov := &stubProvider{openFiles: []string{"src/a.go", "src/b.go", "src/c.go"}}
	mw := NewOpenFilesMiddleware(prov, nil)
	pc := &PromptContext{SessionID: "s1", PromptCount: 0}
	action, msg := mw.BeforePrompt(nil, pc)
	if action != ActionInject {
		t.Fatalf("expected ActionInject, got %v", action)
	}
	if !strings.Contains(msg, "## Open Files") {
		t.Errorf("expected open files header, got %q", msg)
	}
	if !strings.Contains(msg, "src/a.go") || !strings.Contains(msg, "src/b.go") {
		t.Errorf("expected paths in message, got %q", msg)
	}
}

func TestOpenFilesMiddleware_CapsAtMax(t *testing.T) {
	sm := DefaultSystemMessages()
	paths := make([]string, sm.MaxOpenFiles+5)
	for i := range paths {
		paths[i] = "file" + strconv.Itoa(i) + ".go"
	}
	prov := &stubProvider{openFiles: paths}
	mw := NewOpenFilesMiddleware(prov, sm)
	_, msg := mw.BeforePrompt(nil, &PromptContext{SessionID: "s1"})
	// Count the bullet lines.
	lines := strings.Count(msg, "- file")
	if lines > sm.MaxOpenFiles {
		t.Errorf("expected at most %d file lines, got %d", sm.MaxOpenFiles, lines)
	}
}

func TestOpenFilesMiddleware_NilProviderSkips(t *testing.T) {
	mw := NewOpenFilesMiddleware(nil, nil)
	action, msg := mw.BeforePrompt(nil, &PromptContext{SessionID: "s1"})
	if action != ActionContinue {
		t.Errorf("expected ActionContinue with nil provider, got %v (%q)", action, msg)
	}
}

func TestRecentEditsMiddleware_SkipsWhenEmpty(t *testing.T) {
	mw := NewRecentEditsMiddleware(&stubProvider{}, nil)
	action, msg := mw.BeforePrompt(nil, &PromptContext{SessionID: "s1"})
	if action != ActionContinue {
		t.Errorf("expected ActionContinue when no recent edits, got %v (%q)", action, msg)
	}
}

func TestRecentEditsMiddleware_InjectsPaths(t *testing.T) {
	prov := &stubProvider{recentEdits: []string{"src/x.go", "src/y.go"}}
	mw := NewRecentEditsMiddleware(prov, nil)
	action, msg := mw.BeforePrompt(nil, &PromptContext{SessionID: "s1"})
	if action != ActionInject {
		t.Fatalf("expected ActionInject, got %v", action)
	}
	if !strings.Contains(msg, "## Recently Edited Files") {
		t.Errorf("expected recent edits header, got %q", msg)
	}
	if !strings.Contains(msg, "src/x.go") {
		t.Errorf("expected path in message, got %q", msg)
	}
}
