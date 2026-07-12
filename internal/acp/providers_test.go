package acp

import (
	"context"
	"strings"
	"testing"

	"github.com/adama/local-agent/internal/interfaces"
	"github.com/adama/local-agent/internal/search"
)

// fileWorkspaceManager is a stub WorkspaceManager that returns canned file
// contents keyed by relative path. Used by OpenFilesResourceMiddleware tests.
type fileWorkspaceManager struct {
	files map[string]string
	err   error // when non-nil, ReadFile returns this error
}

func (m *fileWorkspaceManager) Register(_ context.Context, path string) (interfaces.WorkspaceInfo, error) {
	return interfaces.WorkspaceInfo{ID: "ws", Path: path, Name: path}, nil
}

func (m *fileWorkspaceManager) List(_ context.Context) ([]interfaces.WorkspaceInfo, error) {
	return nil, nil
}

func (m *fileWorkspaceManager) FileTree(_ context.Context, _ string) ([]interfaces.FileNode, error) {
	return nil, nil
}

func (m *fileWorkspaceManager) ReadFile(_ context.Context, _, relPath string) (string, int64, bool, error) {
	if m.err != nil {
		return "", 0, false, m.err
	}
	content, ok := m.files[relPath]
	if !ok {
		return "", 0, false, &fileNotFoundError{path: relPath}
	}
	return content, 1, false, nil
}

func (m *fileWorkspaceManager) Search(_ context.Context, _, _ string, _ search.Options) ([]search.Result, error) {
	return nil, nil
}

func (m *fileWorkspaceManager) FilePath(_ context.Context, _, _ string) (string, error) {
	return "", &fileNotFoundError{path: "not implemented"}
}

func (m *fileWorkspaceManager) Remove(_ context.Context, _ string) error { return nil }

// fileNotFoundError is a minimal sentinel error so ReadFile failures are
// distinguishable in tests.
type fileNotFoundError struct{ path string }

func (e *fileNotFoundError) Error() string { return "file not found: " + e.path }

func TestOpenFilesResourceMiddleware_ReadsFileContents(t *testing.T) {
	tr := NewOpenFilesTracker()
	tr.SetOpenFiles([]string{"a.go", "b.ts"})
	wm := &fileWorkspaceManager{files: map[string]string{
		"a.go": "package main\n",
		"b.ts": "export const x = 1\n",
	}}
	mw := NewOpenFilesResourceMiddleware(tr, wm, nil)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: "/ws"}

	resources := mw.BeforePromptResources(context.Background(), pc)
	if len(resources) != 2 {
		t.Fatalf("expected 2 resources, got %d: %+v", len(resources), resources)
	}
	if resources[0].Name != "a.go" {
		t.Errorf("expected first resource name a.go, got %q", resources[0].Name)
	}
	if resources[0].Text != "package main\n" {
		t.Errorf("expected a.go content, got %q", resources[0].Text)
	}
	if resources[0].MimeType != "text/x-go" {
		t.Errorf("expected a.go mime text/x-go, got %q", resources[0].MimeType)
	}
	if !strings.HasPrefix(resources[0].URI, "file:///ws/") {
		t.Errorf("expected a.go URI to start with file:///ws/, got %q", resources[0].URI)
	}
	if resources[1].Name != "b.ts" {
		t.Errorf("expected second resource name b.ts, got %q", resources[1].Name)
	}
	if resources[1].MimeType != "text/typescript" {
		t.Errorf("expected b.ts mime text/typescript, got %q", resources[1].MimeType)
	}
}

func TestOpenFilesResourceMiddleware_SkipsMissingFiles(t *testing.T) {
	tr := NewOpenFilesTracker()
	tr.SetOpenFiles([]string{"a.go", "missing.go"})
	wm := &fileWorkspaceManager{files: map[string]string{"a.go": "package main\n"}}
	mw := NewOpenFilesResourceMiddleware(tr, wm, nil)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: "/ws"}

	resources := mw.BeforePromptResources(context.Background(), pc)
	if len(resources) != 1 {
		t.Fatalf("expected 1 resource (missing file skipped), got %d", len(resources))
	}
	if resources[0].Name != "a.go" {
		t.Errorf("expected a.go resource, got %q", resources[0].Name)
	}
}

func TestOpenFilesResourceMiddleware_RespectsMaxOpenFiles(t *testing.T) {
	tr := NewOpenFilesTracker()
	tr.SetOpenFiles([]string{"a.go", "b.go", "c.go", "d.go"})
	wm := &fileWorkspaceManager{files: map[string]string{
		"a.go": "a", "b.go": "b", "c.go": "c", "d.go": "d",
	}}
	sm := DefaultSystemMessages()
	sm.MaxOpenFiles = 2
	mw := NewOpenFilesResourceMiddleware(tr, wm, sm)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: "/ws"}

	resources := mw.BeforePromptResources(context.Background(), pc)
	if len(resources) != 2 {
		t.Fatalf("expected 2 resources (capped at MaxOpenFiles), got %d", len(resources))
	}
	if resources[0].Name != "a.go" || resources[1].Name != "b.go" {
		t.Errorf("expected a.go and b.go, got %q and %q", resources[0].Name, resources[1].Name)
	}
}

func TestOpenFilesResourceMiddleware_PerFileByteCap(t *testing.T) {
	tr := NewOpenFilesTracker()
	tr.SetOpenFiles([]string{"big.go"})
	wm := &fileWorkspaceManager{files: map[string]string{
		"big.go": strings.Repeat("x", 1000),
	}}
	sm := DefaultSystemMessages()
	sm.MaxOpenFileBytes = 100
	mw := NewOpenFilesResourceMiddleware(tr, wm, sm)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: "/ws"}

	resources := mw.BeforePromptResources(context.Background(), pc)
	if len(resources) != 1 {
		t.Fatalf("expected 1 resource, got %d", len(resources))
	}
	if len(resources[0].Text) != 100 {
		t.Errorf("expected truncated to 100 bytes, got %d", len(resources[0].Text))
	}
}

func TestOpenFilesResourceMiddleware_AggregateByteCapStopsEarly(t *testing.T) {
	tr := NewOpenFilesTracker()
	tr.SetOpenFiles([]string{"a.go", "b.go", "c.go"})
	wm := &fileWorkspaceManager{files: map[string]string{
		"a.go": strings.Repeat("a", 60),
		"b.go": strings.Repeat("b", 60),
		"c.go": strings.Repeat("c", 60),
	}}
	sm := DefaultSystemMessages()
	// Per-file cap large enough not to interfere; aggregate cap stops after b.go
	// (60 + 60 = 120 >= 100).
	sm.MaxOpenFileBytes = 10 * 1024
	sm.MaxOpenFilesTotalBytes = 100
	mw := NewOpenFilesResourceMiddleware(tr, wm, sm)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: "/ws"}

	resources := mw.BeforePromptResources(context.Background(), pc)
	if len(resources) != 2 {
		t.Fatalf("expected 2 resources (aggregate cap stops before c.go), got %d", len(resources))
	}
	if resources[0].Name != "a.go" || resources[1].Name != "b.go" {
		t.Errorf("expected a.go and b.go, got %q and %q", resources[0].Name, resources[1].Name)
	}
}

func TestOpenFilesResourceMiddleware_EmitsSelectionResource(t *testing.T) {
	tr := NewOpenFilesTracker()
	tr.SetOpenFiles([]string{"a.go"})
	wm := &fileWorkspaceManager{files: map[string]string{"a.go": "package main\n"}}
	tr.SetSelection(EditorSelection{
		Path:      "a.go",
		StartLine: 2,
		EndLine:   4,
		Text:      "selected text",
	})
	mw := NewOpenFilesResourceMiddleware(tr, wm, nil)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: "/ws"}

	resources := mw.BeforePromptResources(context.Background(), pc)
	// 1 file resource + 1 selection resource.
	if len(resources) != 2 {
		t.Fatalf("expected 2 resources (file + selection), got %d", len(resources))
	}
	sel := resources[1]
	if sel.Text != "selected text" {
		t.Errorf("expected selection text, got %q", sel.Text)
	}
	if !strings.Contains(sel.URI, "#L2-L4") {
		t.Errorf("expected selection URI to contain #L2-L4, got %q", sel.URI)
	}
	if sel.Name != "a.go:2-4" {
		t.Errorf("expected selection name a.go:2-4, got %q", sel.Name)
	}
}

func TestOpenFilesResourceMiddleware_EmptySelectionNotEmitted(t *testing.T) {
	tr := NewOpenFilesTracker()
	tr.SetOpenFiles([]string{"a.go"})
	wm := &fileWorkspaceManager{files: map[string]string{"a.go": "package main\n"}}
	// Empty-text selection should not produce a selection resource.
	tr.SetSelection(EditorSelection{Path: "a.go", StartLine: 1, EndLine: 1, Text: ""})
	mw := NewOpenFilesResourceMiddleware(tr, wm, nil)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: "/ws"}

	resources := mw.BeforePromptResources(context.Background(), pc)
	if len(resources) != 1 {
		t.Fatalf("expected 1 resource (file only, no selection), got %d", len(resources))
	}
}

func TestOpenFilesResourceMiddleware_NilDepsNoPanic(t *testing.T) {
	// Nil tracker — should return nil, not panic.
	mw := NewOpenFilesResourceMiddleware(nil, &fileWorkspaceManager{}, nil)
	resources := mw.BeforePromptResources(context.Background(), &PromptContext{SessionID: "s1"})
	if resources != nil {
		t.Errorf("expected nil resources with nil tracker, got %v", resources)
	}
	// Nil workspace — should return nil, not panic.
	mw2 := NewOpenFilesResourceMiddleware(NewOpenFilesTracker(), nil, nil)
	resources2 := mw2.BeforePromptResources(context.Background(), &PromptContext{SessionID: "s1"})
	if resources2 != nil {
		t.Errorf("expected nil resources with nil workspace, got %v", resources2)
	}
}

func TestOpenFilesResourceMiddleware_BeforePromptContinues(t *testing.T) {
	mw := NewOpenFilesResourceMiddleware(NewOpenFilesTracker(), &fileWorkspaceManager{}, nil)
	action, msg := mw.BeforePrompt(context.Background(), &PromptContext{SessionID: "s1"})
	if action != ActionContinue {
		t.Errorf("expected ActionContinue, got %v", action)
	}
	if msg != "" {
		t.Errorf("expected empty message, got %q", msg)
	}
}

func TestMimeByExt(t *testing.T) {
	cases := map[string]string{
		"a.go":     "text/x-go",
		"a.ts":     "text/typescript",
		"a.tsx":    "text/typescript",
		"a.js":     "text/javascript",
		"a.jsx":    "text/javascript",
		"a.py":     "text/x-python",
		"a.md":     "text/markdown",
		"a.json":   "application/json",
		"a.yaml":   "text/yaml",
		"a.yml":    "text/yaml",
		"a.html":   "text/html",
		"a.css":    "text/css",
		"a.txt":    "text/plain",
		"Makefile": "text/plain",
	}
	for path, want := range cases {
		if got := mimeByExt(path); got != want {
			t.Errorf("mimeByExt(%q) = %q, want %q", path, got, want)
		}
	}
}

func TestOpenFilesTracker_Selection(t *testing.T) {
	tr := NewOpenFilesTracker()
	if sel := tr.Selection(); sel.Text != "" {
		t.Errorf("expected empty selection initially, got %+v", sel)
	}
	tr.SetSelection(EditorSelection{Path: "a.go", StartLine: 1, EndLine: 2, Text: "hi"})
	sel := tr.Selection()
	if sel.Path != "a.go" || sel.StartLine != 1 || sel.EndLine != 2 || sel.Text != "hi" {
		t.Errorf("unexpected selection: %+v", sel)
	}
	tr.SetSelection(EditorSelection{})
	if sel := tr.Selection(); sel.Text != "" || sel.Path != "" {
		t.Errorf("expected cleared selection, got %+v", sel)
	}
}
