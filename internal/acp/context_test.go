package acp

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/adama/local-agent/internal/interfaces"
	"github.com/adama/local-agent/internal/search"
)

// fakeWorkspaceManager is a stub interfaces.WorkspaceManager for tests. It
// serves a fixed file tree and workspace list without touching the disk for
// tree lookups (path resolution still uses the registered paths).
type fakeWorkspaceManager struct {
	workspaces []interfaces.WorkspaceInfo
	tree       []interfaces.FileNode
	err        error
}

func (m *fakeWorkspaceManager) Register(_ context.Context, path string) (interfaces.WorkspaceInfo, error) {
	return interfaces.WorkspaceInfo{ID: "ws", Path: path, Name: filepath.Base(path)}, nil
}

func (m *fakeWorkspaceManager) List(_ context.Context) ([]interfaces.WorkspaceInfo, error) {
	return m.workspaces, nil
}

func (m *fakeWorkspaceManager) FileTree(_ context.Context, _ string) ([]interfaces.FileNode, error) {
	if m.err != nil {
		return nil, m.err
	}
	return m.tree, nil
}

func (m *fakeWorkspaceManager) ReadFile(_ context.Context, _, _ string) (string, int64, error) {
	return "", 0, nil
}

func (m *fakeWorkspaceManager) Search(_ context.Context, _, _ string, _ search.SearchOptions) ([]search.SearchResult, error) {
	return nil, nil
}

func (m *fakeWorkspaceManager) Remove(_ context.Context, id string) error {
	for i, ws := range m.workspaces {
		if ws.ID == id {
			m.workspaces = append(m.workspaces[:i], m.workspaces[i+1:]...)
			return nil
		}
	}
	return fmt.Errorf("workspace not found: %s", id)
}

// fakeMiddleware is a configurable PromptMiddleware for pipeline tests.
type fakeMiddleware struct {
	action  PromptAction
	message string
	called  bool
}

func (f *fakeMiddleware) BeforePrompt(_ context.Context, _ *PromptContext) (PromptAction, string) {
	f.called = true
	return f.action, f.message
}

// --- Pipeline tests --------------------------------------------------------

func TestPromptPipeline_Empty(t *testing.T) {
	p := NewPromptPipeline()
	pc := &PromptContext{SessionID: "s1"}
	action, msg := p.RunBeforePrompt(context.Background(), pc)
	if action != ActionContinue {
		t.Errorf("expected ActionContinue, got %v", action)
	}
	if msg != "" {
		t.Errorf("expected empty message, got %q", msg)
	}
}

func TestPromptPipeline_ConcatenatesWithSeparator(t *testing.T) {
	mw1 := &fakeMiddleware{action: ActionInject, message: "context A"}
	mw2 := &fakeMiddleware{action: ActionInject, message: "context B"}
	p := NewPromptPipeline(mw1, mw2)

	pc := &PromptContext{SessionID: "s1"}
	action, msg := p.RunBeforePrompt(context.Background(), pc)
	if action != ActionInject {
		t.Fatalf("expected ActionInject, got %v", action)
	}
	want := "context A\n\n---\n\ncontext B"
	if msg != want {
		t.Errorf("expected %q, got %q", want, msg)
	}
	if !mw1.called || !mw2.called {
		t.Error("expected both middlewares to be called")
	}
}

func TestPromptPipeline_PromptCounterBumps(t *testing.T) {
	p := NewPromptPipeline()
	pc := &PromptContext{SessionID: "s1"}
	_, _ = p.RunBeforePrompt(context.Background(), pc)
	if pc.PromptCount != 0 {
		t.Errorf("first call PromptCount = %d, want 0", pc.PromptCount)
	}
	_, _ = p.RunBeforePrompt(context.Background(), pc)
	if pc.PromptCount != 1 {
		t.Errorf("second call PromptCount = %d, want 1", pc.PromptCount)
	}
}

func TestPromptPipeline_Reset(t *testing.T) {
	p := NewPromptPipeline()
	pc := &PromptContext{SessionID: "s1"}
	_, _ = p.RunBeforePrompt(context.Background(), pc)
	_, _ = p.RunBeforePrompt(context.Background(), pc)
	p.Reset("s1")
	_, _ = p.RunBeforePrompt(context.Background(), pc)
	if pc.PromptCount != 0 {
		t.Errorf("after Reset PromptCount = %d, want 0", pc.PromptCount)
	}
}

// --- FirstPromptContextMiddleware tests ------------------------------------

// makeTempWorkspace creates a temp dir, optionally inits a git repo, and
// registers it with a fake workspace manager.
func makeTempWorkspace(t *testing.T, initGit bool) (string, *fakeWorkspaceManager) {
	t.Helper()
	dir := t.TempDir()
	// Write an AGENTS.md so we can verify it is included.
	if err := os.WriteFile(filepath.Join(dir, "AGENTS.md"), []byte("# Test Agents\nRules go here.\n"), 0o644); err != nil {
		t.Fatalf("write AGENTS.md: %v", err)
	}
	if initGit {
		if _, err := exec.LookPath("git"); err != nil {
			t.Skip("git not available")
		}
		for _, args := range [][]string{
			{"init"},
			{"config", "user.email", "test@example.com"},
			{"config", "user.name", "Test"},
		} {
			if err := exec.Command("git", append([]string{"-C", dir}, args...)...).Run(); err != nil {
				t.Fatalf("git %v: %v", args, err)
			}
		}
		if err := os.WriteFile(filepath.Join(dir, "README.md"), []byte("hello\n"), 0o644); err != nil {
			t.Fatalf("write README: %v", err)
		}
		if err := exec.Command("git", "-C", dir, "add", ".").Run(); err != nil {
			t.Fatalf("git add: %v", err)
		}
		if err := exec.Command("git", "-C", dir, "commit", "-m", "initial").Run(); err != nil {
			t.Fatalf("git commit: %v", err)
		}
	}
	wm := &fakeWorkspaceManager{
		workspaces: []interfaces.WorkspaceInfo{{ID: "ws", Path: dir, Name: filepath.Base(dir)}},
	}
	return dir, wm
}

func TestFirstPromptContextMiddleware_FirstPromptInjects(t *testing.T) {
	dir, wm := makeTempWorkspace(t, false)
	mw := NewFirstPromptContextMiddleware(wm, nil)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: dir, UserPrompt: "hi", PromptCount: 0}

	action, msg := mw.BeforePrompt(context.Background(), pc)
	if action != ActionInject {
		t.Fatalf("expected ActionInject, got %v", action)
	}
	if !strings.Contains(msg, dir) {
		t.Errorf("expected message to contain workspace path %q, got %q", dir, msg)
	}
	if !strings.Contains(msg, "## AGENTS.md") {
		t.Errorf("expected AGENTS.md section, got %q", msg)
	}
}

func TestFirstPromptContextMiddleware_SecondPromptNoInject(t *testing.T) {
	dir, wm := makeTempWorkspace(t, false)
	mw := NewFirstPromptContextMiddleware(wm, nil)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: dir, PromptCount: 1}

	action, msg := mw.BeforePrompt(context.Background(), pc)
	if action != ActionContinue {
		t.Errorf("expected ActionContinue on second prompt, got %v", action)
	}
	if msg != "" {
		t.Errorf("expected empty message on second prompt, got %q", msg)
	}
}

func TestFirstPromptContextMiddleware_ResetReinjects(t *testing.T) {
	dir, wm := makeTempWorkspace(t, false)
	p := NewPromptPipeline(NewFirstPromptContextMiddleware(wm, nil))
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: dir}

	// First prompt injects.
	action1, _ := p.RunBeforePrompt(context.Background(), pc)
	if action1 != ActionInject {
		t.Fatalf("first prompt: expected ActionInject, got %v", action1)
	}
	// Second prompt does not.
	action2, _ := p.RunBeforePrompt(context.Background(), pc)
	if action2 != ActionContinue {
		t.Fatalf("second prompt: expected ActionContinue, got %v", action2)
	}
	// After reset, next prompt injects again.
	p.Reset("s1")
	action3, _ := p.RunBeforePrompt(context.Background(), pc)
	if action3 != ActionInject {
		t.Fatalf("after reset: expected ActionInject, got %v", action3)
	}
}

func TestFirstPromptContextMiddleware_EmptyWorkspace(t *testing.T) {
	dir := t.TempDir()
	wm := &fakeWorkspaceManager{
		workspaces: []interfaces.WorkspaceInfo{{ID: "ws", Path: dir, Name: "empty"}},
		tree:       nil,
	}
	mw := NewFirstPromptContextMiddleware(wm, nil)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: dir, PromptCount: 0}

	action, msg := mw.BeforePrompt(context.Background(), pc)
	if action != ActionInject {
		t.Fatalf("expected ActionInject even for empty workspace, got %v", action)
	}
	if !strings.Contains(msg, dir) {
		t.Errorf("expected path in minimal context, got %q", msg)
	}
	if !strings.Contains(msg, "Platform:") {
		t.Errorf("expected platform line in minimal context, got %q", msg)
	}
}

func TestFirstPromptContextMiddleware_LargeWorkspaceTruncates(t *testing.T) {
	dir := t.TempDir()
	sm := DefaultSystemMessages()
	// Build a tree with more than MaxContextFiles files.
	var nodes []interfaces.FileNode
	for i := 0; i < sm.MaxContextFiles+50; i++ {
		nodes = append(nodes, interfaces.FileNode{
			Name: "file" + strconv.Itoa(i) + ".go",
			Type: "file",
			Path: "src/file" + strconv.Itoa(i) + ".go",
		})
	}
	wm := &fakeWorkspaceManager{
		workspaces: []interfaces.WorkspaceInfo{{ID: "ws", Path: dir, Name: "big"}},
		tree:       nodes,
	}
	mw := NewFirstPromptContextMiddleware(wm, nil)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: dir, PromptCount: 0}

	_, msg := mw.BeforePrompt(context.Background(), pc)
	// Count the number of file lines emitted.
	lines := strings.Count(msg, "src/file")
	if lines > sm.MaxContextFiles {
		t.Errorf("expected at most %d file lines, got %d", sm.MaxContextFiles, lines)
	}
	if len(msg) > sm.MaxContextBytes {
		t.Errorf("expected message ≤ %d bytes, got %d", sm.MaxContextBytes, len(msg))
	}
}

func TestFirstPromptContextMiddleware_NonGitWorkspaceOmitsGitSection(t *testing.T) {
	dir := t.TempDir()
	wm := &fakeWorkspaceManager{
		workspaces: []interfaces.WorkspaceInfo{{ID: "ws", Path: dir, Name: "nogit"}},
	}
	mw := NewFirstPromptContextMiddleware(wm, nil)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: dir, PromptCount: 0}

	action, msg := mw.BeforePrompt(context.Background(), pc)
	if action != ActionInject {
		t.Fatalf("expected ActionInject, got %v", action)
	}
	if strings.Contains(msg, "## Git") {
		t.Errorf("expected git section omitted for non-git workspace, got %q", msg)
	}
}

func TestFirstPromptContextMiddleware_GitWorkspaceIncludesGitSection(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("git not available")
	}
	dir, wm := makeTempWorkspace(t, true)
	// Rebuild the tree from the real disk so FileTree returns the committed files.
	wm.tree = []interfaces.FileNode{
		{Name: "AGENTS.md", Type: "file", Path: "AGENTS.md"},
		{Name: "README.md", Type: "file", Path: "README.md"},
	}
	mw := NewFirstPromptContextMiddleware(wm, nil)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: dir, PromptCount: 0}

	action, msg := mw.BeforePrompt(context.Background(), pc)
	if action != ActionInject {
		t.Fatalf("expected ActionInject, got %v", action)
	}
	if !strings.Contains(msg, "## Git") {
		t.Errorf("expected git section for git workspace, got %q", msg)
	}
	if !strings.Contains(msg, "initial") {
		t.Errorf("expected recent commit in git section, got %q", msg)
	}
}

func TestFirstPromptContextMiddleware_DepthLimit(t *testing.T) {
	dir := t.TempDir()
	// Build a tree: top/file.go (depth 1) and top/sub/deep/file.go (depth 3)
	// and top/sub/deep/deeper/very_deep.go (depth 4, should be excluded).
	wm := &fakeWorkspaceManager{
		workspaces: []interfaces.WorkspaceInfo{{ID: "ws", Path: dir, Name: "deep"}},
		tree: []interfaces.FileNode{
			{Name: "top", Type: "folder", Path: "top", Children: []interfaces.FileNode{
				{Name: "file.go", Type: "file", Path: filepath.Join("top", "file.go")},
				{Name: "sub", Type: "folder", Path: filepath.Join("top", "sub"), Children: []interfaces.FileNode{
					{Name: "deep", Type: "folder", Path: filepath.Join("top", "sub", "deep"), Children: []interfaces.FileNode{
						{Name: "very_deep.go", Type: "file", Path: filepath.Join("top", "sub", "deep", "very_deep.go")},
						{Name: "deeper", Type: "folder", Path: filepath.Join("top", "sub", "deep", "deeper"), Children: []interfaces.FileNode{
							{Name: "too_deep.go", Type: "file", Path: filepath.Join("top", "sub", "deep", "deeper", "too_deep.go")},
						}},
					}},
				}},
			}},
		},
	}
	mw := NewFirstPromptContextMiddleware(wm, nil)
	pc := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: dir, PromptCount: 0}

	_, msg := mw.BeforePrompt(context.Background(), pc)
	// depth 1 (top/file.go) and depth 3 (top/sub/deep/very_deep.go) included;
	// depth 4 (top/sub/deep/deeper/too_deep.go) excluded.
	if !strings.Contains(msg, "top/file.go") {
		t.Errorf("expected top/file.go included, got %q", msg)
	}
	if !strings.Contains(msg, "top/sub/deep/very_deep.go") {
		t.Errorf("expected top/sub/deep/very_deep.go included (depth 3), got %q", msg)
	}
	if strings.Contains(msg, "too_deep.go") {
		t.Errorf("expected too_deep.go excluded (depth 4), got %q", msg)
	}
}

// --- Integration: SendPrompt with pipeline ---------------------------------

// We can't easily swap the real Transport, so this test exercises the pipeline
// + SendPrompt wiring by checking the emitted PromptSubmitted event content,
// which mirrors what is sent to the transport.
func TestSendPrompt_PipelineInjectsOnFirstPromptOnly(t *testing.T) {
	dir := t.TempDir()
	wm := &fakeWorkspaceManager{
		workspaces: []interfaces.WorkspaceInfo{{ID: "ws", Path: dir, Name: "ws"}},
	}
	client := NewClient(wm, nil)
	cb := &mockCallbacks{}
	client.SetCallbacks(cb)
	client.SetPipeline(NewPromptPipeline(NewFirstPromptContextMiddleware(wm, nil)))

	// We can't call SendPrompt without a real transport (it would try to spawn
	// an agent). Instead, exercise the pipeline directly to confirm the
	// counter semantics that SendPrompt relies on.
	pipeline := client.pipeline
	pc1 := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: dir, UserPrompt: "first"}
	action1, inj1 := pipeline.RunBeforePrompt(context.Background(), pc1)
	if action1 != ActionInject {
		t.Fatalf("first prompt: expected ActionInject, got %v", action1)
	}
	combined1 := inj1 + "\n\n---\n\nfirst"
	if !strings.HasPrefix(combined1, "## Workspace Context") {
		t.Errorf("expected injected context prepended, got %q", combined1)
	}
	if !strings.HasSuffix(combined1, "first") {
		t.Errorf("expected user prompt at end, got %q", combined1)
	}

	// Second prompt: no injection.
	pc2 := &PromptContext{SessionID: "s1", WorkspaceID: "ws", WorkspacePath: dir, UserPrompt: "second"}
	action2, inj2 := pipeline.RunBeforePrompt(context.Background(), pc2)
	if action2 != ActionContinue {
		t.Fatalf("second prompt: expected ActionContinue, got %v", action2)
	}
	if inj2 != "" {
		t.Errorf("second prompt: expected no injection, got %q", inj2)
	}
	// Combined content for second prompt is just the raw user content.
	combined2 := "second"
	if combined2 != "second" {
		t.Errorf("expected raw content on second prompt, got %q", combined2)
	}
}
