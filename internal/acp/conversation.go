// Package acp conversation transfer: export a session's event history as a
// markdown transcript and inject it as context when a conversation is rebound
// to a different agent harness mid-chat.
//
// When a user switches the agent/model for an existing conversation, the live
// ACP transport is torn down and a fresh one starts on the next prompt. The new
// agent has no memory of the prior exchange. ExportConversation reads the
// session's event log and renders a compact markdown transcript; the
// ConversationTransferMiddleware queues that transcript so it is injected into
// the first prompt sent to the new agent, giving it enough context to continue
// the conversation. The transcript is truncated to a configurable byte limit so
// a long history does not blow past the agent's context window.

package acp

import (
	"context"
	"fmt"
	"strings"
	"sync"

	"github.com/adama/local-agent/internal/interfaces"
)

// exportEventQueryLimit is the maximum number of events pulled from the store
// when exporting a conversation. It is intentionally large so a full history is
// captured; truncation to the byte budget happens after rendering.
const exportEventQueryLimit = 10000

// ExportConversation formats a session's event history as a markdown
// conversation. It reads events from the event store and renders user prompts
// and assistant responses (including tool call summaries) as a readable
// transcript. The output is truncated to maxBytes if it exceeds that limit.
//
// The transcript interleaves user messages, assistant text, and compact tool
// summaries in chronological order:
//
//	**User:** Can you fix the bug in auth.go?
//
//	**Assistant:** I'll look at auth.go...
//	[Tool: read_file]
//	**Assistant:** The fix is to add a mutex...
//
//	**User:** Great, now add tests.
//
// Internal events (connection restarts, file writes, permission prompts, shell
// output streaming, plan updates) are skipped to keep the transcript focused on
// the conversational exchange. When maxBytes > 0 and the rendered transcript
// exceeds it, the output is cut to maxBytes and a truncation note is appended.
// When maxBytes <= 0 no truncation is applied.
func ExportConversation(ctx context.Context, store interfaces.EventStore, sessionID string, maxBytes int) (string, error) {
	if store == nil {
		return "", nil
	}
	// Fetch the full event history for the session. afterID=0 retrieves from the
	// beginning; the large limit ensures we capture everything in one pass.
	events, err := store.Query(ctx, sessionID, 0, exportEventQueryLimit)
	if err != nil {
		return "", fmt.Errorf("query session events: %w", err)
	}

	var b strings.Builder
	var pendingAssistant strings.Builder

	// flushAssistant writes any accumulated assistant text as a single
	// **Assistant:** block and resets the accumulator.
	flushAssistant := func() {
		text := strings.TrimSpace(pendingAssistant.String())
		pendingAssistant.Reset()
		if text == "" {
			return
		}
		if b.Len() > 0 {
			b.WriteString("\n\n")
		}
		fmt.Fprintf(&b, "**Assistant:** %s", text)
	}

	for _, e := range events {
		switch e.Type {
		case interfaces.EventPromptSubmitted:
			// Flush any pending assistant text before starting a new user turn.
			flushAssistant()
			if b.Len() > 0 {
				b.WriteString("\n\n")
			}
			fmt.Fprintf(&b, "**User:** %s", strings.TrimSpace(e.Content))

		case interfaces.EventStreamUpdate:
			// Skip the terminal empty StreamUpdate (streaming=false, content="")
			// that signals response completion, and skip thought chunks which
			// are internal reasoning not part of the visible transcript.
			if e.Thought {
				continue
			}
			if e.Content == "" {
				continue
			}
			// Accumulate assistant text chunks; flush happens on the next
			// user/tool boundary.
			if pendingAssistant.Len() > 0 {
				pendingAssistant.WriteString(" ")
			}
			pendingAssistant.WriteString(strings.TrimSpace(e.Content))

		case interfaces.EventToolStarted:
			// Emit a compact one-line tool summary. Flush pending assistant text
			// first so tool calls appear between assistant turns.
			flushAssistant()
			if b.Len() > 0 {
				b.WriteString("\n")
			}
			name := e.Tool
			if name == "" {
				name = e.ToolKind
			}
			if name == "" {
				name = "tool"
			}
			fmt.Fprintf(&b, "[Tool: %s]", name)

		case interfaces.EventToolCompleted:
			// Skip — the ToolStarted line already marks the tool call. Including
			// the completion would duplicate the entry; the summary is often
			// verbose and would bloat the transcript past the byte budget.

		default:
			// Skip internal/non-conversational events: ResponseStarted (the
			// "Agent is thinking…" indicator), ConnectionRestarted, FileWritten,
			// FileRevisionUpdated, SessionCancelled, SessionInterrupted,
			// AgentExited, PermissionRequested/Granted/Denied,
			// ShellCommandStarted/OutputStreamed/Completed, PlanUpdated,
			// SessionResumed.
		}
	}
	// Flush any trailing assistant text.
	flushAssistant()

	out := b.String()
	if maxBytes > 0 && len(out) > maxBytes {
		total := len(out)
		// Reserve room for the truncation note so the final output fits the
		// budget. The note format is fixed below.
		note := fmt.Sprintf("\n\n[... conversation truncated, %d bytes total ...]", total)
		out = out[:maxBytes-len(note)] + note
	}
	return out, nil
}

// ConversationTransferMiddleware injects a previously-exported conversation
// transcript into the first prompt of a session after it has been rebound to a
// new agent. RebindSession exports the prior conversation and calls
// SetTransfer to queue it; the next SendPrompt runs the pipeline, this
// middleware fires on PromptCount == 0, injects the transcript under the
// configured ConversationTransferHeader, and clears the queue so subsequent
// prompts do not re-inject it.
//
// The middleware is safe for concurrent use.
type ConversationTransferMiddleware struct {
	Messages *SystemMessages

	mu        sync.Mutex
	transfers map[string]conversationTransfer
}

// conversationTransfer is a queued transcript awaiting injection on the next
// first prompt of a session.
type conversationTransfer struct {
	Markdown      string
	FromAgentName string
}

// NewConversationTransferMiddleware constructs a
// ConversationTransferMiddleware backed by the given system-message templates.
// If messages is nil, DefaultSystemMessages is used.
func NewConversationTransferMiddleware(messages *SystemMessages) *ConversationTransferMiddleware {
	m := &ConversationTransferMiddleware{
		Messages:  messages,
		transfers: make(map[string]conversationTransfer),
	}
	m.ensureMessages()
	return m
}

// ensureMessages returns the configured SystemMessages, falling back to
// defaults. It memoizes the default so subsequent calls return the same
// instance without re-checking nil.
func (m *ConversationTransferMiddleware) ensureMessages() *SystemMessages {
	if m.Messages == nil {
		m.Messages = DefaultSystemMessages()
	}
	return m.Messages
}

// SetTransfer queues a conversation transcript for injection into the next
// first prompt (PromptCount == 0) of the given session. It is called by
// RebindSession after exporting the prior conversation and before the new agent
// receives its first prompt. fromAgentName is substituted into the
// ConversationTransferHeader template's {agentName} placeholder.
func (m *ConversationTransferMiddleware) SetTransfer(sessionID, conversationMarkdown, fromAgentName string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.transfers[sessionID] = conversationTransfer{
		Markdown:      conversationMarkdown,
		FromAgentName: fromAgentName,
	}
}

// BeforePrompt implements PromptMiddleware. On the first prompt after a rebind
// (PromptCount == 0), if a transfer is queued for the session, it injects the
// transcript under the ConversationTransferHeader (with {agentName} filled in)
// and clears the queue. On any other prompt the queued transfer is cleared
// without injection (the first-prompt window has passed).
func (m *ConversationTransferMiddleware) BeforePrompt(_ context.Context, pc *PromptContext) (PromptAction, string) {
	m.mu.Lock()
	transfer, ok := m.transfers[pc.SessionID]
	if ok {
		// Clear the queue regardless of whether we inject — the transfer is
		// only valid for the first prompt after a rebind.
		delete(m.transfers, pc.SessionID)
	}
	m.mu.Unlock()

	if !ok || pc.PromptCount != 0 || strings.TrimSpace(transfer.Markdown) == "" {
		return ActionContinue, ""
	}

	sm := m.ensureMessages()
	header := sm.Render(sm.ConversationTransferHeader, map[string]string{
		"agentName": transfer.FromAgentName,
	})
	body := fmt.Sprintf("%s\n\n%s", header, transfer.Markdown)
	return ActionInject, body
}
