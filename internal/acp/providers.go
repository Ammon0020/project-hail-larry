// Package acp context providers: event-driven middlewares for time, open
// files, and recent edits.
//
// These middlewares extend the prompt pipeline with lightweight, per-prompt
// context that changes between prompts (unlike FirstPromptContextMiddleware,
// which only fires on the first prompt). Each reads its data from a provider or
// the system clock and injects a small section. Sections are skipped when there
// is nothing to report so the agent never receives empty headers.
//
// All header strings and numeric limits come from the shared SystemMessages
// templates so they remain customizable via configs/system-messages.json.

package acp

import (
	"context"
	"fmt"
	"path/filepath"
	"strings"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
)

// MIME types for the open-file context resources. Centralised so the producer
// (mimeByExt) and consumers/tests reference the same literals.
const (
	mimeTextGo         = "text/x-go"
	mimeTextTypeScript = "text/typescript"
	mimeTextJavaScript = "text/javascript"
	mimeTextMarkdown   = "text/markdown"
	mimeTextYAML       = "text/yaml"
	mimeTextPlain      = "text/plain"
)

// OpenFilesProvider supplies the currently open and recently edited file paths
// (relative to the workspace root) reported by the frontend. Implementations
// must be safe for concurrent use since the pipeline may run from multiple
// goroutines.
type OpenFilesProvider interface {
	// OpenFiles returns the paths of files currently open in the editor, or an
	// empty/nil slice when none are reported.
	OpenFiles() []string
	// RecentEdits returns the paths of files recently edited, or an empty/nil
	// slice when none are reported.
	RecentEdits() []string
}

// nowFunc is the clock used by TimeMiddleware. It is a function variable so
// tests can stub it; production uses time.Now.
var nowFunc = time.Now

// TimeMiddleware injects the current time (ISO 8601 with timezone) on EVERY
// prompt. This is cheap and gives the agent a sense of when "now" is, which
// helps with time-relative reasoning (e.g. "today", "recently").
type TimeMiddleware struct {
	Messages *SystemMessages
}

// NewTimeMiddleware constructs a TimeMiddleware using the given templates. If
// messages is nil, DefaultSystemMessages is used.
func NewTimeMiddleware(messages *SystemMessages) *TimeMiddleware {
	if messages == nil {
		messages = DefaultSystemMessages()
	}
	return &TimeMiddleware{Messages: messages}
}

// BeforePrompt implements PromptMiddleware. It always injects the current time.
func (m *TimeMiddleware) BeforePrompt(_ context.Context, _ *PromptContext) (PromptAction, string) {
	sm := m.Messages
	if sm == nil {
		sm = DefaultSystemMessages()
	}
	now := nowFunc()
	// ISO 8601 with timezone, e.g. "2026-06-27T15:04:05-07:00".
	body := fmt.Sprintf("%s\n\n%s", sm.TimeHeader, now.Format(time.RFC3339))
	return ActionInject, body
}

// OpenFilesMiddleware injects the list of currently open file paths on every
// prompt. Open files can change between prompts, so this fires each time. When
// the provider returns no paths, injection is skipped (no empty section).
type OpenFilesMiddleware struct {
	Provider OpenFilesProvider
	Messages *SystemMessages
}

// NewOpenFilesMiddleware constructs an OpenFilesMiddleware backed by the given
// provider and templates. If messages is nil, DefaultSystemMessages is used.
func NewOpenFilesMiddleware(provider OpenFilesProvider, messages *SystemMessages) *OpenFilesMiddleware {
	if messages == nil {
		messages = DefaultSystemMessages()
	}
	return &OpenFilesMiddleware{Provider: provider, Messages: messages}
}

// BeforePrompt implements PromptMiddleware. It injects the open-file list when
// the provider reports at least one path.
func (m *OpenFilesMiddleware) BeforePrompt(_ context.Context, _ *PromptContext) (PromptAction, string) {
	if m.Provider == nil {
		return ActionContinue, ""
	}
	sm := m.Messages
	if sm == nil {
		sm = DefaultSystemMessages()
	}
	paths := capPaths(m.Provider.OpenFiles(), sm.MaxOpenFiles)
	if len(paths) == 0 {
		return ActionContinue, ""
	}
	var b strings.Builder
	fmt.Fprintf(&b, "%s\n\n", sm.OpenFilesHeader)
	for _, p := range paths {
		fmt.Fprintf(&b, "- %s\n", p)
	}
	return ActionInject, strings.TrimSpace(b.String())
}

// RecentEditsMiddleware injects the list of recently edited file paths on every
// prompt. Like open files, this fires each prompt since the edit set changes.
// When the provider returns no paths, injection is skipped.
type RecentEditsMiddleware struct {
	Provider OpenFilesProvider
	Messages *SystemMessages
}

// NewRecentEditsMiddleware constructs a RecentEditsMiddleware backed by the
// given provider and templates. If messages is nil, DefaultSystemMessages is
// used.
func NewRecentEditsMiddleware(provider OpenFilesProvider, messages *SystemMessages) *RecentEditsMiddleware {
	if messages == nil {
		messages = DefaultSystemMessages()
	}
	return &RecentEditsMiddleware{Provider: provider, Messages: messages}
}

// BeforePrompt implements PromptMiddleware. It injects the recent-edits list
// when the provider reports at least one path.
func (m *RecentEditsMiddleware) BeforePrompt(_ context.Context, _ *PromptContext) (PromptAction, string) {
	if m.Provider == nil {
		return ActionContinue, ""
	}
	sm := m.Messages
	if sm == nil {
		sm = DefaultSystemMessages()
	}
	paths := capPaths(m.Provider.RecentEdits(), sm.MaxRecentEdits)
	if len(paths) == 0 {
		return ActionContinue, ""
	}
	var b strings.Builder
	fmt.Fprintf(&b, "%s\n\n", sm.RecentEditsHeader)
	for _, p := range paths {
		fmt.Fprintf(&b, "- %s\n", p)
	}
	return ActionInject, strings.TrimSpace(b.String())
}

// capPaths returns at most the first n entries of paths. When n is zero or
// negative the full slice is returned (no cap).
func capPaths(paths []string, n int) []string {
	if n <= 0 || len(paths) <= n {
		return paths
	}
	return paths[:n]
}

// OpenFilesResourceMiddleware sends open file contents and the current editor
// selection as structured resource blocks with every prompt. Unlike the
// text-only OpenFilesMiddleware (which sends a path list), this reads each
// file's content via the WorkspaceManager and emits it as a ContextResource
// for agents that support embedded context. The text-only middleware still
// runs alongside it so agents without EmbeddedContext get at least the path
// list via the transport's text fallback.
//
// Resources are capped two ways to keep prompt size bounded:
//   - MaxOpenFileBytes truncates any single file's content.
//   - MaxOpenFilesTotalBytes caps the aggregate; once exceeded, remaining
//     files are skipped.
//
// The current editor selection (when it has text) is emitted as its own
// resource with a fragment URI of the form file://...#L{start}-L{end}.
type OpenFilesResourceMiddleware struct {
	Tracker   *OpenFilesTracker
	Workspace interfaces.WorkspaceManager
	Messages  *SystemMessages
}

// NewOpenFilesResourceMiddleware constructs an OpenFilesResourceMiddleware
// backed by the given tracker, workspace manager, and templates. If messages
// is nil, DefaultSystemMessages is used.
func NewOpenFilesResourceMiddleware(tracker *OpenFilesTracker, wm interfaces.WorkspaceManager, messages *SystemMessages) *OpenFilesResourceMiddleware {
	if messages == nil {
		messages = DefaultSystemMessages()
	}
	return &OpenFilesResourceMiddleware{Tracker: tracker, Workspace: wm, Messages: messages}
}

// BeforePrompt implements PromptMiddleware. This middleware contributes only
// structured resources (no free-form text), so it always returns
// ActionContinue. The pipeline picks up its resources via BeforePromptResources.
func (m *OpenFilesResourceMiddleware) BeforePrompt(_ context.Context, _ *PromptContext) (PromptAction, string) {
	return ActionContinue, ""
}

// BeforePromptResources implements ResourceMiddleware. It reads each open
// file's content from the workspace and emits a ContextResource per file,
// plus one resource for the current editor selection when it has text.
func (m *OpenFilesResourceMiddleware) BeforePromptResources(ctx context.Context, pc *PromptContext) []ContextResource {
	if m.Tracker == nil || m.Workspace == nil {
		return nil
	}
	sm := m.Messages
	if sm == nil {
		sm = DefaultSystemMessages()
	}
	paths := capPaths(m.Tracker.OpenFiles(), sm.MaxOpenFiles)
	if len(paths) == 0 && pc.WorkspaceID == "" {
		// No open files and (without a workspace) no way to resolve a
		// selection either — bail out early.
		return nil
	}

	var resources []ContextResource
	total := 0
	for _, rel := range paths {
		content, _, _, err := m.Workspace.ReadFile(ctx, pc.WorkspaceID, rel)
		if err != nil || content == "" {
			continue
		}
		// Skip binary files — null bytes in content would break JSON-RPC
		// serialization ("embedded null byte" error from the agent SDK).
		if strings.ContainsRune(content, 0) {
			continue
		}
		if sm.MaxOpenFileBytes > 0 && len(content) > sm.MaxOpenFileBytes {
			content = content[:sm.MaxOpenFileBytes]
		}
		// Stop adding files once the aggregate cap is exceeded. We still
		// include the file that pushed us over (so the agent sees at least
		// one file when only one is open), but skip everything after it.
		total += len(content)
		resources = append(resources, ContextResource{
			URI:      "file://" + filepath.ToSlash(filepath.Join(pc.WorkspacePath, rel)),
			MimeType: mimeByExt(rel),
			Name:     rel,
			Text:     content,
		})
		if sm.MaxOpenFilesTotalBytes > 0 && total >= sm.MaxOpenFilesTotalBytes {
			break
		}
	}

	// Append the current editor selection as its own resource when it has
	// text. The URI carries a #L{start}-L{end} fragment so the agent can
	// reference the selection range.
	if sel := m.Tracker.Selection(); sel.Text != "" {
		uri := "file://" + filepath.ToSlash(filepath.Join(pc.WorkspacePath, sel.Path)) +
			fmt.Sprintf("#L%d-L%d", sel.StartLine, sel.EndLine)
		resources = append(resources, ContextResource{
			URI:      uri,
			MimeType: mimeByExt(sel.Path),
			Name:     fmt.Sprintf("%s:%d-%d", sel.Path, sel.StartLine, sel.EndLine),
			Text:     sel.Text,
		})
	}

	return resources
}

// mimeByExt returns a MIME type for common source file extensions. Unknown
// extensions default to text/plain. The mapping is intentionally small and
// focused on the languages the editor highlights.
func mimeByExt(path string) string {
	ext := strings.ToLower(filepath.Ext(path))
	switch ext {
	case ".go":
		return mimeTextGo
	case ".ts", ".tsx":
		return mimeTextTypeScript
	case ".js", ".jsx":
		return mimeTextJavaScript
	case ".py":
		return "text/x-python"
	case ".md":
		return mimeTextMarkdown
	case ".json":
		return "application/json"
	case ".yaml", ".yml":
		return mimeTextYAML
	case ".html":
		return "text/html"
	case ".css":
		return "text/css"
	default:
		return mimeTextPlain
	}
}
