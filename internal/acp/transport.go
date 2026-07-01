package acp

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"sync"

	"github.com/adama/local-agent/internal/interfaces"
	"github.com/coder/acp-go-sdk"
)

// acpClientImpl implements the coder/acp-go-sdk Client interface.
type acpClientImpl struct {
	callbacks     interfaces.ACPCallbacks
	workspaceMgr  interfaces.WorkspaceManager
	permMgr       interfaces.PermissionManager
	workspaceID   string
	workspacePath string // absolute path to the workspace dir (for shell exec)
	sessionID     string // our internal session ID

	termMu    sync.Mutex
	terminals map[string]*terminalEntry
}

func (c *acpClientImpl) SessionUpdate(_ context.Context, params acp.SessionNotification) error {
	u := params.Update
	if c.callbacks == nil {
		return nil
	}

	switch {
	case u.AgentMessageChunk != nil:
		if u.AgentMessageChunk.Content.Text != nil {
			c.callbacks.OnEvent(interfaces.Event{
				Type:      interfaces.EventStreamUpdate,
				SessionID: c.sessionID,
				Role:      "agent",
				Content:   u.AgentMessageChunk.Content.Text.Text,
				Streaming: true,
			})
		}
	case u.AgentThoughtChunk != nil:
		if u.AgentThoughtChunk.Content.Text != nil {
			c.callbacks.OnEvent(interfaces.Event{
				Type:      interfaces.EventStreamUpdate,
				SessionID: c.sessionID,
				Role:      "agent",
				Content:   u.AgentThoughtChunk.Content.Text.Text,
				Streaming: true,
				Thought:   true,
			})
		}
	case u.ToolCall != nil:
		target := ""
		if len(u.ToolCall.Locations) > 0 {
			target = u.ToolCall.Locations[0].Path
		}
		c.callbacks.OnEvent(interfaces.Event{
			Type:       interfaces.EventToolStarted,
			SessionID:  c.sessionID,
			Tool:       u.ToolCall.Title,
			ToolKind:   string(u.ToolCall.Kind),
			ToolCallID: string(u.ToolCall.ToolCallId),
			Target:     target,
			Command:    rawInputString(u.ToolCall.RawInput),
			Summary:    string(u.ToolCall.Status),
		})
	case u.ToolCallUpdate != nil:
		status := ""
		if u.ToolCallUpdate.Status != nil {
			status = string(*u.ToolCallUpdate.Status)
		}
		kind := ""
		if u.ToolCallUpdate.Kind != nil {
			kind = string(*u.ToolCallUpdate.Kind)
		}
		target := ""
		if len(u.ToolCallUpdate.Locations) > 0 {
			target = u.ToolCallUpdate.Locations[0].Path
		}
		c.callbacks.OnEvent(interfaces.Event{
			Type:       interfaces.EventToolCompleted,
			SessionID:  c.sessionID,
			ToolCallID: string(u.ToolCallUpdate.ToolCallId),
			ToolKind:   kind,
			Target:     target,
			Summary:    status,
			Content:    toolContentSummary(u.ToolCallUpdate.Content),
		})
	case u.Plan != nil:
		c.callbacks.OnEvent(interfaces.Event{
			Type:      interfaces.EventPlanUpdated,
			SessionID: c.sessionID,
			Content:   planSummary(u.Plan.Entries),
		})
	case u.UserMessageChunk != nil:
		// Usually already emitted by our side when the prompt was submitted.
	}
	return nil
}

// toolContentSummary renders tool call content blocks (text or diffs) into a
// compact string for display in tool cards.
func toolContentSummary(blocks []acp.ToolCallContent) string {
	parts := make([]string, 0, len(blocks))
	for i := range blocks {
		b := blocks[i]
		switch {
		case b.Diff != nil:
			old := ""
			if b.Diff.OldText != nil {
				old = *b.Diff.OldText
			}
			parts = append(parts, fmt.Sprintf("--- %s\n%s\n+++\n%s", b.Diff.Path, old, b.Diff.NewText))
		case b.Content != nil && b.Content.Content.Text != nil:
			parts = append(parts, b.Content.Content.Text.Text)
		case b.Terminal != nil:
			parts = append(parts, "[terminal "+b.Terminal.TerminalId+"]")
		}
	}
	return joinNonEmpty(parts, "\n")
}

// planSummary renders plan entries as a checklist string ("status: content").
func planSummary(entries []acp.PlanEntry) string {
	lines := make([]string, 0, len(entries))
	for _, e := range entries {
		lines = append(lines, string(e.Status)+": "+e.Content)
	}
	return joinNonEmpty(lines, "\n")
}

// joinNonEmpty joins the non-empty members of parts with sep.
func joinNonEmpty(parts []string, sep string) string {
	out := ""
	for _, p := range parts {
		if p == "" {
			continue
		}
		if out != "" {
			out += sep
		}
		out += p
	}
	return out
}

func (c *acpClientImpl) RequestPermission(ctx context.Context, params acp.RequestPermissionRequest) (acp.RequestPermissionResponse, error) {
	if c.permMgr == nil {
		return acp.RequestPermissionResponse{}, fmt.Errorf("permission manager not configured")
	}

	// The agent's ToolCallUpdate.Title is optional (*string). Many agents omit
	// it in permission requests, so falling back to the raw ToolCallId would
	// surface an opaque random ID (e.g. "muNNhDHjd") as the prompt's primary
	// label — the "Permission Required / muNNhDHjd" bug. Instead, prefer the
	// agent-supplied title; when absent, synthesize a human-readable label from
	// the tool kind so the UI always shows a meaningful action description.
	title := ""
	if params.ToolCall.Title != nil && *params.ToolCall.Title != "" && !looksLikeRawID(*params.ToolCall.Title) {
		title = *params.ToolCall.Title
	} else {
		// Title missing, or the agent supplied an opaque tool-call ID as its
		// title (e.g. "toolu_01H…", "call_abc123", a UUID, or "muNNhDHjd").
		// Synthesize a human-readable label from the tool kind instead.
		title = permissionTitleFromKind(params.ToolCall.Kind)
	}

	// Extract the ACP tool kind (execute/edit/read/...) for the UI so it can
	// pick icons and styling consistent with tool cards.
	toolKind := ""
	if params.ToolCall.Kind != nil {
		toolKind = string(*params.ToolCall.Kind)
	}

	// Surface the command (raw input) and target (first affected location) so
	// the UI can show exactly what the agent wants to do.
	command := rawInputString(params.ToolCall.RawInput)
	target := ""
	if len(params.ToolCall.Locations) > 0 {
		target = params.ToolCall.Locations[0].Path
	}

	// The agent supplies its own option set. We echo each option's id back when
	// the user chooses it; OptionDetails carries the labels/kinds for the UI.
	opts := make([]interfaces.PermissionDecision, 0, len(params.Options))
	details := make([]interfaces.PermissionOptionInfo, 0, len(params.Options))
	optMap := make(map[interfaces.PermissionDecision]string)
	for _, o := range params.Options {
		dec := interfaces.PermissionDecision(o.OptionId)
		opts = append(opts, dec)
		details = append(details, interfaces.PermissionOptionInfo{
			ID:   string(o.OptionId),
			Name: o.Name,
			Kind: string(o.Kind),
		})
		optMap[dec] = string(o.OptionId)
	}

	req := interfaces.PermissionRequest{
		SessionID:     c.sessionID,
		Tool:          title,
		ToolKind:      toolKind,
		Command:       command,
		Target:        target,
		Options:       opts,
		OptionDetails: details,
	}

	decision, err := c.permMgr.Request(ctx, req)
	if err != nil {
		// Context cancelled or timed out — cancel the tool call rather than
		// leaving the agent waiting.
		return acp.RequestPermissionResponse{
			Outcome: acp.RequestPermissionOutcome{
				Cancelled: &acp.RequestPermissionOutcomeCancelled{},
			},
		}, nil
	}

	optID, ok := optMap[decision]
	if !ok {
		return acp.RequestPermissionResponse{}, fmt.Errorf("unknown decision chosen: %s", decision)
	}

	return acp.RequestPermissionResponse{
		Outcome: acp.RequestPermissionOutcome{
			Selected: &acp.RequestPermissionOutcomeSelected{
				OptionId: acp.PermissionOptionId(optID),
			},
		},
	}, nil
}

// permissionTitleFromKind synthesizes a human-readable action label from an
// ACP tool kind when the agent did not supply a Title in its permission
// request. This prevents the raw ToolCallId (an opaque ID like "muNNhDHjd")
// from being shown to the user as the permission prompt's primary label.
func permissionTitleFromKind(kind *acp.ToolKind) string {
	if kind == nil {
		return "Tool call"
	}
	switch string(*kind) {
	case "execute":
		return "Run command"
	case "edit":
		return "Edit file"
	case "read":
		return "Read file"
	case "search":
		return "Search"
	case "delete":
		return "Delete file"
	case "move":
		return "Move file"
	default:
		return "Tool call"
	}
}

// Patterns used by looksLikeRawID to recognize opaque generated identifiers.
var (
	// toolCallIDPrefixRe matches well-known agent tool-call ID prefixes followed
	// by an opaque alphanumeric token: Claude's "toolu_", OpenAI's "call_"/"fc_",
	// and generic "tooluse"/"tool_use"/"toolcall" forms.
	toolCallIDPrefixRe = regexp.MustCompile(`(?i)^(toolu|tooluse|tool_use|toolcall|call|fc)[_-][A-Za-z0-9]+$`)
	// uuidRe matches a UUID with or without hyphen separators.
	uuidRe = regexp.MustCompile(`(?i)^[0-9a-f]{8}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{12}$`)
	// longHexRe matches long opaque hex tokens (e.g. SHA-style IDs).
	longHexRe = regexp.MustCompile(`(?i)^[0-9a-f]{16,}$`)
	// shortAlnumIDRe matches a separator-free short alphanumeric token, the
	// classic random-ID shape (e.g. "muNNhDHjd").
	shortAlnumIDRe = regexp.MustCompile(`^[A-Za-z0-9]{1,24}$`)
)

// looksLikeRawID reports whether s looks like an opaque generated identifier
// rather than a human-readable action label. Some agents set a permission
// request's ToolCall.Title to the raw tool-call ID; showing that to the user
// ("Permission Required / toolu_01H…") is meaningless, so callers fall back to
// a kind-derived label instead. The frontend mirrors this heuristic as a
// defensive safety net (see ChatMessageItem.tsx#looksLikeRawId).
func looksLikeRawID(s string) bool {
	s = strings.TrimSpace(s)
	if s == "" {
		return true
	}
	// Multi-word, human-readable labels (containing whitespace) are never IDs.
	if strings.ContainsAny(s, " \t\n") {
		return false
	}
	switch {
	case toolCallIDPrefixRe.MatchString(s):
		return true
	case uuidRe.MatchString(s):
		return true
	case longHexRe.MatchString(s):
		return true
	case shortAlnumIDRe.MatchString(s):
		return true
	default:
		return false
	}
}

// toRelativePath converts a path that may be absolute (within the workspace)
// or already relative into a workspace-relative path. Agents often send
// absolute paths (e.g. "C:\Users\...\readme.md") but our workspace manager
// requires relative paths and rejects absolutes in safeJoin.
func toRelativePath(workspacePath, p string) string {
	cleaned := filepath.Clean(p)
	if !filepath.IsAbs(cleaned) {
		return cleaned
	}
	rel, err := filepath.Rel(workspacePath, cleaned)
	if err != nil {
		return cleaned
	}
	return rel
}

// rawInputString renders an ACP tool call's raw input as a human-readable
// string for display in permission prompts and tool cards. JSON objects are
// compacted; plain strings pass through.
func rawInputString(raw any) string {
	if raw == nil {
		return ""
	}
	if s, ok := raw.(string); ok {
		return s
	}
	b, err := json.Marshal(raw)
	if err != nil {
		return ""
	}
	return string(b)
}

func (c *acpClientImpl) ReadTextFile(ctx context.Context, params acp.ReadTextFileRequest) (acp.ReadTextFileResponse, error) {
	if c.workspaceMgr == nil {
		return acp.ReadTextFileResponse{}, fmt.Errorf("workspace manager not configured")
	}
	relPath := toRelativePath(c.workspacePath, params.Path)
	content, _, err := c.workspaceMgr.ReadFile(ctx, c.workspaceID, relPath)
	if err != nil {
		return acp.ReadTextFileResponse{}, err
	}
	return acp.ReadTextFileResponse{Content: content}, nil
}

func (c *acpClientImpl) WriteTextFile(ctx context.Context, params acp.WriteTextFileRequest) (acp.WriteTextFileResponse, error) {
	if c.workspaceMgr == nil {
		return acp.WriteTextFileResponse{}, fmt.Errorf("workspace manager not configured")
	}
	// Agent writes don't use optimistic locking — pass expectedRevision=0.
	// The file-sync layer tracks revisions for user-facing edits; agent writes
	// are tracked via FileRevisionUpdated events emitted by the daemon.
	type fileWriter interface {
		WriteFile(ctx context.Context, workspaceID, relPath, content string, expectedRevision int64) (int64, error)
	}
	fw, ok := c.workspaceMgr.(fileWriter)
	if !ok {
		return acp.WriteTextFileResponse{}, fmt.Errorf("workspace manager does not support writing")
	}
	relPath := toRelativePath(c.workspacePath, params.Path)
	if _, err := fw.WriteFile(ctx, c.workspaceID, relPath, params.Content, 0); err != nil {
		return acp.WriteTextFileResponse{}, err
	}
	// Broadcast a file-written event so the frontend can refresh its file tree
	// and show the newly created/modified file without a manual reload. The
	// workspace ID lets the UI refresh only the affected workspace's tree.
	if c.callbacks != nil {
		c.callbacks.OnEvent(interfaces.Event{
			Type:        interfaces.EventFileWritten,
			SessionID:   c.sessionID,
			WorkspaceID: c.workspaceID,
			Target:      relPath,
		})
	}
	return acp.WriteTextFileResponse{}, nil
}

// Terminal methods are implemented in terminal.go.

// Transport manages a single agent process connection.
type Transport struct {
	cmd    *exec.Cmd
	conn   *acp.ClientSideConnection
	stderr *ringBuffer
	cwd    string // workspace cwd captured at Start, reused for LoadSession
}

// NewTransport returns a new Transport that manages a single agent process
// connection. The returned Transport is not started; call Start to launch the
// agent subprocess.
func NewTransport() *Transport {
	return &Transport{}
}

// StderrTail returns the most recent captured agent stderr, for inclusion in
// failure diagnostics.
func (t *Transport) StderrTail() string {
	if t.stderr == nil {
		return ""
	}
	return t.stderr.String()
}

// Start launches the agent subprocess with the given command and args in
// workdir, wiring its stdin/stdout to a new ACP client-side connection backed
// by impl. Agent stderr is captured into a bounded ring buffer for diagnostics.
func (t *Transport) Start(ctx context.Context, command string, args []string, workdir string, impl *acpClientImpl) error {
	t.cmd = exec.CommandContext(ctx, command, args...) //nolint:gosec // command/args originate from the trusted agent registry (autodetect), not user input
	t.cmd.Dir = workdir
	// Remember the workspace cwd so LoadSession can re-supply it without the
	// caller having to pass it back in (the ACP LoadSessionRequest requires it).
	t.cwd = workdir
	// Capture agent stderr into a bounded ring buffer instead of inheriting the
	// daemon's stderr. This keeps daemon logs clean and lets us surface the tail
	// of agent diagnostics when a session fails.
	t.stderr = newRingBuffer(8 << 10) // 8 KiB
	t.cmd.Stderr = t.stderr
	stdin, err := t.cmd.StdinPipe()
	if err != nil {
		return fmt.Errorf("stdin pipe: %w", err)
	}
	stdout, err := t.cmd.StdoutPipe()
	if err != nil {
		return fmt.Errorf("stdout pipe: %w", err)
	}
	if err := t.cmd.Start(); err != nil {
		return fmt.Errorf("start: %w", err)
	}

	t.conn = acp.NewClientSideConnection(impl, stdin, stdout)
	// Suppress ACP SDK diagnostic logging (e.g. "connection closed") —
	// these are noisy during normal operation and not actionable.
	t.conn.SetLogger(slog.New(slog.NewTextHandler(io.Discard, nil)))
	return nil
}

// Initialize performs the ACP initialize handshake, advertising the client's
// capabilities (filesystem read/write and terminal support) and returning the
// agent's capabilities and info.
func (t *Transport) Initialize(ctx context.Context) (acp.InitializeResponse, error) {
	return t.conn.Initialize(ctx, acp.InitializeRequest{
		ClientInfo: &acp.Implementation{
			Name:    "LocalAgentInterface",
			Version: "1.0",
		},
		// Advertise what we actually support so capability-checking agents
		// (per the ACP spec they MUST treat omitted capabilities as false)
		// will use our filesystem and terminal methods.
		ClientCapabilities: acp.ClientCapabilities{
			Fs: acp.FileSystemCapabilities{
				ReadTextFile:  true,
				WriteTextFile: true,
			},
			Terminal: true,
		},
	})
}

// NewSession asks the agent to create a new ACP session rooted at cwd and
// returns the new session ID.
func (t *Transport) NewSession(ctx context.Context, cwd string) (string, error) {
	result, err := t.conn.NewSession(ctx, acp.NewSessionRequest{
		Cwd:        cwd,
		McpServers: []acp.McpServer{},
	})
	if err != nil {
		return "", err
	}
	return string(result.SessionId), nil
}

// LoadSession asks the agent to resume a previously-created ACP session by ID.
// This only succeeds when the agent advertised the loadSession capability in its
// InitializeResponse; otherwise the agent returns an error and the caller should
// fall back to NewSession. The workspace cwd captured at Start is re-supplied
// (the ACP LoadSessionRequest requires it). Returns the loaded ACP session ID
// (the same value passed in as acpSessionID) on success.
func (t *Transport) LoadSession(ctx context.Context, acpSessionID string) (string, error) {
	_, err := t.conn.LoadSession(ctx, acp.LoadSessionRequest{
		SessionId:  acp.SessionId(acpSessionID),
		Cwd:        t.cwd,
		McpServers: []acp.McpServer{},
	})
	if err != nil {
		return "", err
	}
	// LoadSession reuses the supplied session ID; echo it back so callers can
	// treat NewSession and LoadSession uniformly.
	return acpSessionID, nil
}

// DeleteSession asks the agent to delete a previously-created ACP session by ID
// (the unstable ACP session/delete method). This is a best-effort call: agents
// that do not support session/delete return a MethodNotFound error, which
// callers should ignore. The process is not affected — call Close to terminate
// the agent subprocess.
func (t *Transport) DeleteSession(ctx context.Context, acpSessionID string) error {
	_, err := t.conn.UnstableDeleteSession(ctx, acp.UnstableDeleteSessionRequest{
		SessionId: acp.SessionId(acpSessionID),
	})
	return err
}

// Prompt sends a user prompt containing content to the given ACP session.
func (t *Transport) Prompt(ctx context.Context, sessionID, content string) error {
	_, err := t.conn.Prompt(ctx, acp.PromptRequest{
		SessionId: acp.SessionId(sessionID),
		Prompt: []acp.ContentBlock{
			acp.TextBlock(content),
		},
	})
	return err
}

// Cancel sends a cancel notification to the given ACP session, requesting the
// agent to abort any in-progress work.
func (t *Transport) Cancel(ctx context.Context, sessionID string) error {
	err := t.conn.Cancel(ctx, acp.CancelNotification{
		SessionId: acp.SessionId(sessionID),
	})
	return err
}

// Close terminates the agent subprocess (if still running) and waits for it
// to exit. It is safe to call even when no process was started.
func (t *Transport) Close() error {
	if t.cmd != nil && t.cmd.Process != nil {
		_ = t.cmd.Process.Kill()
		_ = t.cmd.Wait()
	}
	return nil
}
