// Package acp message templates: externalized system-message configuration.
//
// SystemMessages holds the customizable header strings and numeric limits used
// by the prompt middleware pipeline. The values are loaded from a JSON config
// file (configs/system-messages.json) so operators can tweak the injected
// context wording and caps without editing Go source. When the config file is
// missing or unreadable, DefaultSystemMessages returns the built-in defaults
// that match the original hardcoded values, so the pipeline always has a
// usable template set.
//
// Header strings may contain {placeholders} (e.g. "{count}", "{depth}") that
// are substituted at injection time via SystemMessages.Render.

package acp

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"sync"

	"log/slog"
)

// SystemMessages mirrors the JSON config in configs/system-messages.json. It
// carries the customizable header templates and numeric limits consumed by the
// prompt middleware pipeline.
type SystemMessages struct {
	// WorkspaceContextHeader is the header for the first-prompt workspace
	// context bundle (root path, platform).
	WorkspaceContextHeader string `json:"workspaceContextHeader"`
	// FilesHeader is the header for the flat file-path list. Supports the
	// {count} and {depth} placeholders.
	FilesHeader string `json:"filesHeader"`
	// GitHeader is the header for the git status section.
	GitHeader string `json:"gitHeader"`
	// AgentsMdHeader is the header for the AGENTS.md content section.
	AgentsMdHeader string `json:"agentsMdHeader"`
	// TimeHeader is the header for the current-time section injected on every
	// prompt by TimeMiddleware.
	TimeHeader string `json:"timeHeader"`
	// OpenFilesHeader is the header for the currently-open-files section
	// injected by OpenFilesMiddleware.
	OpenFilesHeader string `json:"openFilesHeader"`
	// RecentEditsHeader is the header for the recently-edited-files section
	// injected by RecentEditsMiddleware.
	RecentEditsHeader string `json:"recentEditsHeader"`
	// ConversationTransferHeader is the header for a transferred-conversation
	// summary. Supports the {agentName} placeholder.
	ConversationTransferHeader string `json:"conversationTransferHeader"`

	// MaxContextBytes caps the total injected context for a single middleware
	// section (applied as a final safety net).
	MaxContextBytes int `json:"maxContextBytes"`
	// MaxContextFiles caps the number of file paths included in the file-tree
	// section.
	MaxContextFiles int `json:"maxContextFiles"`
	// MaxFileTreeDepth limits how deep the file-tree walk descends.
	MaxFileTreeDepth int `json:"maxFileTreeDepth"`
	// MaxOpenFiles caps the number of open-file paths injected by
	// OpenFilesMiddleware.
	MaxOpenFiles int `json:"maxOpenFiles"`
	// MaxRecentEdits caps the number of recently-edited file paths injected by
	// RecentEditsMiddleware.
	MaxRecentEdits int `json:"maxRecentEdits"`
	// MaxOpenFileBytes caps the byte size of a single open file's content
	// emitted as a resource block by OpenFilesResourceMiddleware. Files larger
	// than this are truncated.
	MaxOpenFileBytes int `json:"maxOpenFileBytes"`
	// MaxOpenFilesTotalBytes caps the aggregate byte size of all open-file
	// resource blocks emitted by OpenFilesResourceMiddleware. Once the running
	// total exceeds this, remaining open files are skipped.
	MaxOpenFilesTotalBytes int `json:"maxOpenFilesTotalBytes"`
}

// DefaultSystemMessages returns a SystemMessages populated with the built-in
// defaults. These match the values that were previously hardcoded in
// context.go so behavior is unchanged when no config file is present.
func DefaultSystemMessages() *SystemMessages {
	return &SystemMessages{
		WorkspaceContextHeader:     "## Workspace Context",
		FilesHeader:                "## Files (first {count}, depth ≤ {depth})",
		GitHeader:                  "## Git",
		AgentsMdHeader:             "## AGENTS.md",
		TimeHeader:                 "## Current Time",
		OpenFilesHeader:            "## Open Files",
		RecentEditsHeader:          "## Recently Edited Files",
		ConversationTransferHeader: "## Previous Conversation (transferred from {agentName})",
		MaxContextBytes:            8 * 1024,
		MaxContextFiles:            200,
		MaxFileTreeDepth:           3,
		MaxOpenFiles:               20,
		MaxRecentEdits:             10,
		MaxOpenFileBytes:           32 * 1024,
		MaxOpenFilesTotalBytes:     128 * 1024,
	}
}

// LoadSystemMessages reads and parses the JSON config at path. If the file is
// missing or cannot be read/parsed, DefaultSystemMessages is returned and the
// error is logged via slog so the pipeline still works with sane defaults. A
// non-nil error is also returned so callers that want to handle the failure
// explicitly can.
func LoadSystemMessages(path string) (*SystemMessages, error) {
	data, err := os.ReadFile(path) //nolint:gosec // path is a configured config file path.
	if err != nil {
		slog.Warn("system-messages: config unreadable, using defaults", "path", path, "err", err)
		return DefaultSystemMessages(), err
	}

	sm := DefaultSystemMessages()
	if err := json.Unmarshal(data, sm); err != nil {
		slog.Warn("system-messages: config invalid JSON, using defaults", "path", path, "err", err)
		return DefaultSystemMessages(), fmt.Errorf("parse system messages: %w", err)
	}
	return sm, nil
}

// Render replaces {placeholder} tokens in header with the corresponding values
// from vars. Unknown placeholders are left intact. It is safe to call with a
// nil vars map.
func (sm *SystemMessages) Render(header string, vars map[string]string) string {
	if vars == nil {
		return header
	}
	out := header
	for k, v := range vars {
		out = strings.ReplaceAll(out, "{"+k+"}", v)
	}
	return out
}

// OpenFilesTracker is an in-memory implementation of OpenFilesProvider. It
// holds the currently open and recently edited file paths (relative to the
// workspace root) reported by the frontend via the REST context endpoint. It
// is safe for concurrent use.
//
// The tracker starts empty; when no files have been reported the middlewares
// that consult it skip injection (no empty sections are emitted).
type OpenFilesTracker struct {
	mu          sync.RWMutex
	openFiles   []string
	recentEdits []string
	selection   EditorSelection
}

// EditorSelection captures the user's current text selection in the editor.
// Path is relative to the workspace root; StartLine/EndLine are 1-based and
// inclusive. Text is the selected text itself (may be empty when the user has
// no active selection, in which case the selection is not emitted).
type EditorSelection struct {
	Path      string
	StartLine int
	EndLine   int
	Text      string
}

// NewOpenFilesTracker constructs an empty OpenFilesTracker.
func NewOpenFilesTracker() *OpenFilesTracker {
	return &OpenFilesTracker{}
}

// SetOpenFiles replaces the set of currently open file paths.
func (t *OpenFilesTracker) SetOpenFiles(paths []string) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.openFiles = append([]string(nil), paths...)
}

// SetRecentEdits replaces the set of recently edited file paths.
func (t *OpenFilesTracker) SetRecentEdits(paths []string) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.recentEdits = append([]string(nil), paths...)
}

// OpenFiles returns a copy of the currently open file paths.
func (t *OpenFilesTracker) OpenFiles() []string {
	t.mu.RLock()
	defer t.mu.RUnlock()
	return append([]string(nil), t.openFiles...)
}

// RecentEdits returns a copy of the recently edited file paths.
func (t *OpenFilesTracker) RecentEdits() []string {
	t.mu.RLock()
	defer t.mu.RUnlock()
	return append([]string(nil), t.recentEdits...)
}

// SetSelection replaces the current editor selection. Pass an EditorSelection
// with an empty Text to clear the selection.
func (t *OpenFilesTracker) SetSelection(sel EditorSelection) {
	t.mu.Lock()
	t.selection = sel
	t.mu.Unlock()
}

// Selection returns the current editor selection.
func (t *OpenFilesTracker) Selection() EditorSelection {
	t.mu.RLock()
	defer t.mu.RUnlock()
	return t.selection
}
