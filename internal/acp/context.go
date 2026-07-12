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
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
)

// Context-resource identifiers and MIME types used by the workspace context
// bundle. Centralised here so the URIs and content types stay consistent
// between the producer (this package) and the tests/assertions elsewhere.
const (
	workspaceContextURI  = "context://workspace"
	contextMimeType      = "text/markdown"
	workspaceContextName = "Workspace Context"
	agentsMDFilename     = "AGENTS.md"
	contextCountKey      = "count"
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
	// EmbeddedContext is true when the agent advertised the embeddedContext
	// prompt capability. Middlewares use this to decide whether to emit
	// structured resource blocks (preferred) or fall back to text injection.
	EmbeddedContext bool
}

// PromptMiddleware is a single stage in the pre-prompt pipeline. Each
// middleware inspects the PromptContext and may return injected context text.
type PromptMiddleware interface {
	// BeforePrompt is called before a prompt is sent to the agent. It returns
	// an action (ActionContinue or ActionInject) and, when ActionInject, the
	// context text to prepend.
	BeforePrompt(ctx context.Context, pc *PromptContext) (PromptAction, string)
}

// ContextResource is a structured piece of injected context that the transport
// renders as an ACP resource ContentBlock (uri, mimeType, text) when the agent
// advertises the embeddedContext capability, or folds into text otherwise.
type ContextResource struct {
	URI      string
	MimeType string
	Text     string
	Name     string
}

// PromptResult is the combined output of the pipeline: free-form injected text
// plus structured resource blocks.
type PromptResult struct {
	Text      string
	Resources []ContextResource
}

// ResourceMiddleware is optionally implemented by a PromptMiddleware that
// contributes structured resource blocks. The pipeline type-asserts each
// middleware for it.
type ResourceMiddleware interface {
	BeforePromptResources(ctx context.Context, pc *PromptContext) []ContextResource
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
// messages with the "\n\n---\n\n" separator. It returns ActionInject with a
// PromptResult whose Text is the combined injected text and whose Resources is
// the concatenated structured resource blocks from any middlewares that
// implement ResourceMiddleware. If no middleware injected text or resources it
// returns ActionContinue with an empty PromptResult.
//
// After a successful run the per-session prompt counter is bumped, so the next
// call for the same session observes an incremented PromptCount.
func (p *PromptPipeline) RunBeforePrompt(ctx context.Context, pc *PromptContext) (PromptAction, PromptResult) {
	// Populate the prompt count from the internal counter so middlewares see a
	// consistent value even if the caller did not set it.
	p.mu.Lock()
	pc.PromptCount = p.counts[pc.SessionID]
	p.mu.Unlock()

	var parts []string
	var resources []ContextResource
	for _, m := range p.middlewares {
		action, msg := m.BeforePrompt(ctx, pc)
		if action == ActionInject && msg != "" {
			parts = append(parts, msg)
		}
		if rm, ok := m.(ResourceMiddleware); ok {
			resources = append(resources, rm.BeforePromptResources(ctx, pc)...)
		}
	}

	// Bump the counter after the run so the next prompt for this session sees
	// an incremented count regardless of whether anything was injected.
	p.mu.Lock()
	p.counts[pc.SessionID]++
	p.mu.Unlock()

	if len(parts) == 0 && len(resources) == 0 {
		return ActionContinue, PromptResult{}
	}
	return ActionInject, PromptResult{
		Text:      strings.Join(parts, "\n\n---\n\n"),
		Resources: resources,
	}
}

// Reset clears the per-session prompt counter so the next prompt for the given
// session is treated as the first (PromptCount == 0) again.
func (p *PromptPipeline) Reset(sessionID string) {
	p.mu.Lock()
	defer p.mu.Unlock()
	delete(p.counts, sessionID)
}

// gitCommandTimeout is the per-git-command timeout. Git operations must
// degrade gracefully and never block the prompt flow.
const gitCommandTimeout = 2 * time.Second

// FirstPromptContextMiddleware injects a workspace context bundle only on the
// first prompt of a session (PromptCount == 0). The bundle contains the
// workspace root path, OS/platform, a flat file-path list, git status, and the
// AGENTS.md content when present.
//
// The header strings and numeric limits (file count, tree depth, byte cap) are
// sourced from the SystemMessages templates so they can be customized via the
// configs/system-messages.json file without editing source. When messages is
// nil, DefaultSystemMessages is used so the middleware always has a usable
// template set.
type FirstPromptContextMiddleware struct {
	WorkspaceManager interfaces.WorkspaceManager
	Messages         *SystemMessages
}

// NewFirstPromptContextMiddleware constructs a FirstPromptContextMiddleware
// backed by the given workspace manager and system-message templates. If
// messages is nil, DefaultSystemMessages is used.
func NewFirstPromptContextMiddleware(wm interfaces.WorkspaceManager, messages *SystemMessages) *FirstPromptContextMiddleware {
	if messages == nil {
		messages = DefaultSystemMessages()
	}
	return &FirstPromptContextMiddleware{WorkspaceManager: wm, Messages: messages}
}

// messages returns the configured SystemMessages, falling back to defaults.
func (m *FirstPromptContextMiddleware) messages() *SystemMessages {
	if m.Messages == nil {
		return DefaultSystemMessages()
	}
	return m.Messages
}

// BeforePrompt implements PromptMiddleware. The workspace bundle is now emitted
// as structured resources via BeforePromptResources (so the transport can send
// it as ACP resource ContentBlocks when the agent advertises embeddedContext,
// or fold it into text otherwise). BeforePrompt therefore returns
// ActionContinue with no text; the transport's text fallback preserves the old
// behavior for agents without EmbeddedContext.
func (m *FirstPromptContextMiddleware) BeforePrompt(_ context.Context, _ *PromptContext) (PromptAction, string) {
	return ActionContinue, ""
}

// BeforePromptResources implements ResourceMiddleware. It emits the workspace
// context bundle (header + file tree + git status) as a single
// context://workspace resource and AGENTS.md as its own file:// resource, but
// only on the first prompt of a session (PromptCount == 0).
func (m *FirstPromptContextMiddleware) BeforePromptResources(ctx context.Context, pc *PromptContext) []ContextResource {
	if pc.PromptCount != 0 {
		return nil
	}

	sm := m.messages()

	// Workspace bundle: header + file tree + git status (NOT AGENTS.md, which
	// gets its own resource below so the agent can address it by file URI).
	var b strings.Builder
	m.writeHeader(&b, pc, sm)
	m.writeFileTree(ctx, &b, pc, sm)
	m.writeGitStatus(&b, pc, sm)
	bundle := strings.TrimSpace(b.String())

	var resources []ContextResource
	if bundle != "" {
		if len(bundle) > sm.MaxContextBytes {
			bundle = bundle[:sm.MaxContextBytes]
		}
		resources = append(resources, ContextResource{
			URI:      workspaceContextURI,
			MimeType: contextMimeType,
			Name:     workspaceContextName,
			Text:     bundle,
		})
	}

	// AGENTS.md as its own resource with a real file URI.
	if pc.WorkspacePath != "" {
		path := filepath.Join(pc.WorkspacePath, agentsMDFilename)
		if data, err := os.ReadFile(path); err == nil { //nolint:gosec // path is built from the registered workspace root.
			text := string(data)
			if len(text) > sm.MaxContextBytes {
				text = text[:sm.MaxContextBytes]
			}
			resources = append(resources, ContextResource{
				URI:      "file://" + filepath.ToSlash(path),
				MimeType: contextMimeType,
				Name:     agentsMDFilename,
				Text:     text,
			})
		}
	}

	return resources
}

// writeHeader emits the workspace root path and OS/platform string.
func (m *FirstPromptContextMiddleware) writeHeader(b *strings.Builder, pc *PromptContext, sm *SystemMessages) {
	fmt.Fprintf(b, "%s\n\n", sm.WorkspaceContextHeader)
	fmt.Fprintf(b, "- Workspace root: %s\n", pc.WorkspacePath)
	fmt.Fprintf(b, "- Platform: %s/%s\n", runtime.GOOS, runtime.GOARCH)
}

// writeFileTree appends a flat file-path list grouped by top-level directory.
// It walks the recursive []FileNode returned by FileTree, capping at
// MaxContextFiles entries and MaxFileTreeDepth levels.
func (m *FirstPromptContextMiddleware) writeFileTree(ctx context.Context, b *strings.Builder, pc *PromptContext, sm *SystemMessages) {
	if m.WorkspaceManager == nil || pc.WorkspaceID == "" {
		return
	}
	nodes, err := m.WorkspaceManager.FileTree(ctx, pc.WorkspaceID)
	if err != nil {
		return
	}

	paths := flattenFileNodes(nodes, 0, sm.MaxFileTreeDepth)
	if len(paths) > sm.MaxContextFiles {
		paths = paths[:sm.MaxContextFiles]
	}
	if len(paths) == 0 {
		return
	}

	header := sm.Render(sm.FilesHeader, map[string]string{
		contextCountKey: strconv.Itoa(len(paths)),
		"depth":         strconv.Itoa(sm.MaxFileTreeDepth),
	})
	fmt.Fprintf(b, "\n%s\n\n", header)
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
func (m *FirstPromptContextMiddleware) writeGitStatus(b *strings.Builder, pc *PromptContext, sm *SystemMessages) {
	if pc.WorkspacePath == "" {
		return
	}
	statusOut := runGit(pc.WorkspacePath, "status", "--short", "-b")
	if statusOut == "" {
		// Not a git repo or git missing — omit the section gracefully.
		return
	}
	logOut := runGit(pc.WorkspacePath, "log", "-5", "--oneline")

	fmt.Fprintf(b, "\n%s\n\n", sm.GitHeader)
	fmt.Fprintf(b, "```\n%s```\n", strings.TrimSpace(statusOut))
	if logOut != "" {
		fmt.Fprintf(b, "\nRecent commits:\n```\n%s```\n", strings.TrimSpace(logOut))
	}
}

// flattenFileNodes walks the recursive FileNode tree depth-first and returns a
// flat list of file paths (folders are not included as entries, only files),
// stopping at the given max depth. Hidden files are already excluded by
// FileTree so no additional filtering is needed.
func flattenFileNodes(nodes []interfaces.FileNode, depth, maxDepth int) []string {
	var out []string
	for _, n := range nodes {
		if n.Type == interfaces.FileNodeTypeFile {
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
	cmd := exec.CommandContext(ctx, "git", full...) //nolint:gosec // G204: arguments are hardcoded git subcommands and the registered workspace path, not user input.
	var stdout bytes.Buffer
	cmd.Stdout = &stdout
	// Discard stderr so git warnings don't pollute logs.
	cmd.Stderr = nil
	if err := cmd.Run(); err != nil {
		return ""
	}
	return stdout.String()
}
