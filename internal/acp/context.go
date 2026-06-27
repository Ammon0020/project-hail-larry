// Package acp context injection: the agent context provider.
//
// Agents receive no workspace context on their first prompt, which forces them
// to discover files via shell round-trips. The prompt middleware pipeline
// defined here injects a compact context bundle (workspace path, OS, file tree,
// git status, AGENTS.md) into the first prompt of each session so the agent can
// start working immediately.
//
// Design reference: docs/plans/agent-context.md. Implementation per the
// execution plan Work Stream 3.

package acp

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
)

// PromptAction is the action a PromptMiddleware requests of the pipeline.
type PromptAction int

const (
	// ActionContinue means the middleware injected nothing and the pipeline
	// should proceed to the next middleware unchanged.
	ActionContinue PromptAction = iota
	// ActionInject means the middleware produced context text to prepend to
	// the user's prompt. The pipeline concatenates all injected messages.
	ActionInject
)

// PromptContext carries the per-prompt information made available to
// middlewares when RunBeforePrompt is invoked.
type PromptContext struct {
	SessionID     string
	WorkspaceID   string
	WorkspacePath string
	UserPrompt    string
	// PromptCount is the zero-based index of this prompt within its session
	// (0 on the very first prompt, 1 on the second, and so on).
	PromptCount int
}

// PromptMiddleware is a single stage in the pre-prompt pipeline. Each
// middleware inspects the PromptContext and may return injected context text.
type PromptMiddleware interface {
	// BeforePrompt is called before a prompt is sent to the agent. It returns
	// an action (ActionContinue or ActionInject) and, when ActionInject, the
	// context text to prepend.
	BeforePrompt(ctx context.Context, pc *PromptContext) (PromptAction, string)
}

// PromptPipeline runs an ordered list of PromptMiddleware stages before each
// prompt, concatenating any injected context with a visual separator.
//
// The pipeline also tracks a per-session prompt counter so middlewares can
// distinguish the first prompt of a session from subsequent ones. The counter
// is bumped internally by RunBeforePrompt after a successful run, so callers
// do not need to manage it.
type PromptPipeline struct {
	middlewares []PromptMiddleware

	mu     sync.Mutex
	counts map[string]int
}

// NewPromptPipeline constructs a PromptPipeline that runs the given
// middlewares in order.
func NewPromptPipeline(middlewares ...PromptMiddleware) *PromptPipeline {
	return &PromptPipeline{
		middlewares: middlewares,
		counts:      make(map[string]int),
	}
}

// RunBeforePrompt runs every middleware in order, concatenating injected
// messages with the "\n\n---\n\n" separator. It returns ActionInject with the
// combined text if any middleware injected content, otherwise ActionContinue.
//
// After a successful run the per-session prompt counter is bumped, so the next
// call for the same session observes an incremented PromptCount.
func (p *PromptPipeline) RunBeforePrompt(ctx context.Context, pc *PromptContext) (PromptAction, string) {
	// Populate the prompt count from the internal counter so middlewares see a
	// consistent value even if the caller did not set it.
	p.mu.Lock()
	pc.PromptCount = p.counts[pc.SessionID]
	p.mu.Unlock()

	var parts []string
	injected := false
	for _, m := range p.middlewares {
		action, msg := m.BeforePrompt(ctx, pc)
		if action == ActionInject && msg != "" {
			parts = append(parts, msg)
			injected = true
		}
	}

	// Bump the counter after the run so the next prompt for this session sees
	// an incremented count regardless of whether anything was injected.
	p.mu.Lock()
	p.counts[pc.SessionID]++
	p.mu.Unlock()

	if !injected {
		return ActionContinue, ""
	}
	return ActionInject, strings.Join(parts, "\n\n---\n\n")
}

// Reset clears the per-session prompt counter so the next prompt for the given
// session is treated as the first (PromptCount == 0) again.
func (p *PromptPipeline) Reset(sessionID string) {
	p.mu.Lock()
	defer p.mu.Unlock()
	delete(p.counts, sessionID)
}

// maxContextFiles caps the number of file paths included in the injected
// context to keep the bundle small.
const maxContextFiles = 200

// maxContextBytes caps the total injected context at roughly 8KB.
const maxContextBytes = 8 * 1024

// maxFileTreeDepth limits how deep into the workspace tree we walk when
// flattening the file list. Depth is measured from the workspace root.
const maxFileTreeDepth = 3

// gitCommandTimeout is the per-git-command timeout. Git operations must
// degrade gracefully and never block the prompt flow.
const gitCommandTimeout = 2 * time.Second

// FirstPromptContextMiddleware injects a workspace context bundle only on the
// first prompt of a session (PromptCount == 0). The bundle contains the
// workspace root path, OS/platform, a flat file-path list, git status, and the
// AGENTS.md content when present.
type FirstPromptContextMiddleware struct {
	WorkspaceManager interfaces.WorkspaceManager
}

// NewFirstPromptContextMiddleware constructs a FirstPromptContextMiddleware
// backed by the given workspace manager.
func NewFirstPromptContextMiddleware(wm interfaces.WorkspaceManager) *FirstPromptContextMiddleware {
	return &FirstPromptContextMiddleware{WorkspaceManager: wm}
}

// BeforePrompt implements PromptMiddleware. It injects only when
// pc.PromptCount == 0.
func (m *FirstPromptContextMiddleware) BeforePrompt(ctx context.Context, pc *PromptContext) (PromptAction, string) {
	if pc.PromptCount != 0 {
		return ActionContinue, ""
	}

	var b strings.Builder
	m.writeHeader(&b, pc)
	m.writeFileTree(ctx, &b, pc)
	m.writeGitStatus(&b, pc)
	m.writeAgentsMD(&b, pc)

	out := strings.TrimSpace(b.String())
	if out == "" {
		return ActionContinue, ""
	}
	// Enforce the global size cap as a final safety net.
	if len(out) > maxContextBytes {
		out = out[:maxContextBytes]
	}
	return ActionInject, out
}

// writeHeader emits the workspace root path and OS/platform string.
func (m *FirstPromptContextMiddleware) writeHeader(b *strings.Builder, pc *PromptContext) {
	fmt.Fprintf(b, "## Workspace Context\n\n")
	fmt.Fprintf(b, "- Workspace root: %s\n", pc.WorkspacePath)
	fmt.Fprintf(b, "- Platform: %s/%s\n", runtime.GOOS, runtime.GOARCH)
}

// writeFileTree appends a flat file-path list grouped by top-level directory.
// It walks the recursive []FileNode returned by FileTree, capping at
// maxContextFiles entries and maxFileTreeDepth levels.
func (m *FirstPromptContextMiddleware) writeFileTree(ctx context.Context, b *strings.Builder, pc *PromptContext) {
	if m.WorkspaceManager == nil || pc.WorkspaceID == "" {
		return
	}
	nodes, err := m.WorkspaceManager.FileTree(ctx, pc.WorkspaceID)
	if err != nil {
		return
	}

	paths := flattenFileNodes(nodes, 0, maxFileTreeDepth)
	if len(paths) > maxContextFiles {
		paths = paths[:maxContextFiles]
	}
	if len(paths) == 0 {
		return
	}

	fmt.Fprintf(b, "\n## Files (first %d, depth ≤ %d)\n\n", len(paths), maxFileTreeDepth)
	// Group by top-level directory for readability.
	groups := groupByTopLevel(paths)
	for _, g := range groups {
		for _, line := range g.lines {
			fmt.Fprintf(b, "%s\n", line)
		}
	}
}

// writeGitStatus appends branch, clean/dirty summary, and the last 5 commits.
// It degrades gracefully (omits the section) if the workspace is not a git
// repo or git is unavailable.
func (m *FirstPromptContextMiddleware) writeGitStatus(b *strings.Builder, pc *PromptContext) {
	if pc.WorkspacePath == "" {
		return
	}
	statusOut := runGit(pc.WorkspacePath, "status", "--short", "-b")
	if statusOut == "" {
		// Not a git repo or git missing — omit the section gracefully.
		return
	}
	logOut := runGit(pc.WorkspacePath, "log", "-5", "--oneline")

	fmt.Fprintf(b, "\n## Git\n\n")
	fmt.Fprintf(b, "```\n%s```\n", strings.TrimSpace(statusOut))
	if logOut != "" {
		fmt.Fprintf(b, "\nRecent commits:\n```\n%s```\n", strings.TrimSpace(logOut))
	}
}

// writeAgentsMD appends the AGENTS.md content if present at the workspace root.
func (m *FirstPromptContextMiddleware) writeAgentsMD(b *strings.Builder, pc *PromptContext) {
	if pc.WorkspacePath == "" {
		return
	}
	path := filepath.Join(pc.WorkspacePath, "AGENTS.md")
	data, err := os.ReadFile(path) //nolint:gosec // path is built from the registered workspace root.
	if err != nil {
		return
	}
	fmt.Fprintf(b, "\n## AGENTS.md\n\n%s\n", string(data))
}

// flattenFileNodes walks the recursive FileNode tree depth-first and returns a
// flat list of file paths (folders are not included as entries, only files),
// stopping at the given max depth. Hidden files are already excluded by
// FileTree so no additional filtering is needed.
func flattenFileNodes(nodes []interfaces.FileNode, depth, maxDepth int) []string {
	var out []string
	for _, n := range nodes {
		if n.Type == "file" {
			out = append(out, n.Path)
			continue
		}
		// Folder: recurse if within depth budget. depth is the folder's own
		// depth; its children live at depth+1. We include files down to
		// maxDepth, so we recurse while depth+1 <= maxDepth (i.e. depth < maxDepth).
		if depth+1 <= maxDepth {
			out = append(out, flattenFileNodes(n.Children, depth+1, maxDepth)...)
		}
	}
	return out
}

// topGroup is an ordered bucket of file paths sharing a top-level directory.
type topGroup struct {
	lines []string
}

// groupByTopLevel groups flat file paths by their top-level directory
// (the first path component), preserving first-seen order of groups.
func groupByTopLevel(paths []string) []topGroup {
	var groups []topGroup
	index := map[string]int{}
	for _, p := range paths {
		top := p
		if idx := strings.IndexByte(p, filepath.Separator); idx >= 0 {
			top = p[:idx]
		}
		// Use forward slashes for stable cross-platform output.
		display := filepath.ToSlash(p)
		if gi, ok := index[top]; ok {
			groups[gi].lines = append(groups[gi].lines, display)
		} else {
			index[top] = len(groups)
			groups = append(groups, topGroup{lines: []string{display}})
		}
	}
	return groups
}

// runGit executes a git command in the given directory with a bounded timeout.
// It returns the trimmed stdout on success, or "" on any failure (missing git,
// non-zero exit, timeout, non-repo). Errors are intentionally swallowed so the
// caller can degrade gracefully.
func runGit(dir string, args ...string) string {
	ctx, cancel := context.WithTimeout(context.Background(), gitCommandTimeout)
	defer cancel()

	full := append([]string{"-C", dir}, args...)
	cmd := exec.CommandContext(ctx, "git", full...)
	var stdout bytes.Buffer
	cmd.Stdout = &stdout
	// Discard stderr so git warnings don't pollute logs.
	cmd.Stderr = nil
	if err := cmd.Run(); err != nil {
		return ""
	}
	return stdout.String()
}
