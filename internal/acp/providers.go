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
	"strings"
	"time"
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
