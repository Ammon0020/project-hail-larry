package acp

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"os"
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

// emitStreamUpdate emits an agent StreamUpdate event with streaming=true.
// `thought` distinguishes agent reasoning (thought chunks) from regular
// message chunks. Used by SessionUpdate for the AgentMessageChunk and
// AgentThoughtChunk update kinds, which differ only in the Thought flag.
func (c *acpClientImpl) emitStreamUpdate(content string, thought bool) {
	c.emit(interfaces.Event{
		Type:      interfaces.EventStreamUpdate,
		SessionID: c.sessionID,
		Role:      "agent",
		Content:   content,
		Streaming: true,
		Thought:   thought,
	})
}

func (c *acpClientImpl) SessionUpdate(_ context.Context, params acp.SessionNotification) error {
	u := params.Update
	if c.callbacks == nil {
		return nil
	}

	switch {
	case u.AgentMessageChunk != nil:
		if u.AgentMessageChunk.Content.Text != nil {
			c.emitStreamUpdate(u.AgentMessageChunk.Content.Text.Text, false)
		}
	case u.AgentThoughtChunk != nil:
		if u.AgentThoughtChunk.Content.Text != nil {
			c.emitStreamUpdate(u.AgentThoughtChunk.Content.Text.Text, true)
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
		content := toolContentSummary(u.ToolCallUpdate.Content)
		// ACP has no structured error field — agents may stash error text in
		// RawOutput instead of Content. Fall back to it so failed tools aren't
		// left with no details.
		if content == "" && u.ToolCallUpdate.RawOutput != nil {
			content = rawInputString(u.ToolCallUpdate.RawOutput)
		}
		// Last-resort: ACP gives us no error field, and some agents report
		// status="failed" with empty content. When we can detect the likely
		// cause on our side (a read outside the workspace boundary), synthesize
		// a clear message so the user isn't left guessing.
		if content == "" && status == "failed" && kind == "read" && target != "" &&
			isOutsideWorkspace(c.workspacePath, target) {
			content = fmt.Sprintf("Read failed: path %q is outside the workspace root %q.", target, c.workspacePath)
		}
		c.callbacks.OnEvent(interfaces.Event{
			Type:       interfaces.EventToolCompleted,
			SessionID:  c.sessionID,
			ToolCallID: string(u.ToolCallUpdate.ToolCallId),
			ToolKind:   kind,
			Target:     target,
			Summary:    status,
			Content:    content,
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

// permissionTitleKindLabels maps an ACP tool kind to a human-readable action
// label, used by permissionTitleFromKind when the agent did not supply a
// Title in its permission request.
var permissionTitleKindLabels = map[string]string{
	"execute": "Run command",
	"edit":    "Edit file",
	"read":    "Read file",
	"search":  "Search",
	"delete":  "Delete file",
	"move":    "Move file",
}

// permissionTitleFromKind synthesizes a human-readable action label from an
// ACP tool kind when the agent did not supply a Title in its permission
// request. This prevents the raw ToolCallId (an opaque ID like "muNNhDHjd")
// from being shown to the user as the permission prompt's primary label.
func permissionTitleFromKind(kind *acp.ToolKind) string {
	if kind == nil {
		return "Tool call"
	}
	if label, ok := permissionTitleKindLabels[string(*kind)]; ok {
		return label
	}
	return "Tool call"
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

// isOutsideWorkspace reports whether path resolves outside the workspace root.
// Used to synthesize a clear error message when an agent reports a failed read
// with no error details and the target is outside the workspace boundary.
func isOutsideWorkspace(workspacePath, path string) bool {
	if path == "" {
		return false
	}
	cleaned := filepath.Clean(path)
	rel, err := filepath.Rel(workspacePath, cleaned)
	if err != nil {
		return true
	}
	if rel == "." {
		return false
	}
	return strings.HasPrefix(rel, "..") || filepath.IsAbs(rel)
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
	c.emit(interfaces.Event{
		Type:        interfaces.EventFileWritten,
		SessionID:   c.sessionID,
		WorkspaceID: c.workspaceID,
		Target:      relPath,
	})
	return acp.WriteTextFileResponse{}, nil
}

// Terminal methods are implemented in terminal.go.

// Transport manages a single agent process connection.
type Transport struct {
	cmd    *exec.Cmd
	conn   *acp.ClientSideConnection
	stderr *ringBuffer
	cwd    string // workspace cwd captured at Start, reused for LoadSession
	// promptCaps holds the agent's advertised prompt capabilities, captured
	// during Initialize. Prompt consults promptCaps.Image to decide whether
	// to send inline image blocks or fall back to resource links + text.
	promptCaps acp.PromptCapabilities
	// mcpServers holds the MCP servers to pass to the agent on session/new and
	// session/load. It is set by the Client (startTransportLocked) after
	// Initialize returns the agent's McpCapabilities, so the list is already
	// filtered to transports the agent advertised support for. nil/empty means
	// "no MCP servers" (the pre-MCP-config behavior).
	mcpServers []acp.McpServer
}

// SetMcpServers sets the MCP server list to pass on the next NewSession /
// LoadSession call. The Client calls this after Initialize so the list can be
// filtered against the agent's advertised McpCapabilities before any session
// RPC is made. It is safe to call before Start; the slice is copied.
func (t *Transport) SetMcpServers(servers []acp.McpServer) {
	t.mcpServers = append(t.mcpServers[:0:0], servers...)
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
	req := acp.InitializeRequest{
		// Pin the protocol version to the SDK-supported v1 value instead of
		// relying on the zero default.
		ProtocolVersion: acp.ProtocolVersionNumber,
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
	}
	if err := req.Validate(); err != nil {
		return acp.InitializeResponse{}, fmt.Errorf("validate initialize request: %w", err)
	}
	return t.conn.Initialize(ctx, req)
}

// NewSession asks the agent to create a new ACP session rooted at cwd and
// returns the new session ID.
func (t *Transport) NewSession(ctx context.Context, cwd string) (string, error) {
	req := acp.NewSessionRequest{
		Cwd:        cwd,
		McpServers: t.mcpServers,
	}
	// The SDK's Validate rejects a nil McpServers slice ("mcpServers is
	// required"), so normalize nil to an empty slice. SetMcpServers leaves a
	// nil slice when never called; this keeps the pre-MCP-config path valid.
	if req.McpServers == nil {
		req.McpServers = []acp.McpServer{}
	}
	if err := req.Validate(); err != nil {
		return "", fmt.Errorf("validate new session request: %w", err)
	}
	result, err := t.conn.NewSession(ctx, req)
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
	req := acp.LoadSessionRequest{
		SessionId:  acp.SessionId(acpSessionID),
		Cwd:        t.cwd,
		McpServers: t.mcpServers,
	}
	// The SDK's Validate rejects a nil McpServers slice ("mcpServers is
	// required"), so normalize nil to an empty slice.
	if req.McpServers == nil {
		req.McpServers = []acp.McpServer{}
	}
	if err := req.Validate(); err != nil {
		return "", fmt.Errorf("validate load session request: %w", err)
	}
	_, err := t.conn.LoadSession(ctx, req)
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
	req := acp.UnstableDeleteSessionRequest{
		SessionId: acp.SessionId(acpSessionID),
	}
	if err := req.Validate(); err != nil {
		return fmt.Errorf("validate delete session request: %w", err)
	}
	_, err := t.conn.UnstableDeleteSession(ctx, req)
	return err
}

// ListSessions asks the agent to list its known sessions, optionally filtered
// by cwd. This is only valid when the agent advertised the
// sessionCapabilities.list capability in its InitializeResponse; callers must
// check initResp.AgentCapabilities.SessionCapabilities.List before invoking.
// Returns the list of SessionInfo entries the agent knows about. Errors from
// agents that do not support session/list should be treated as "no data" by
// callers (the reconcile path falls back to the LoadSession-then-NewSession
// flow). The cwd filter is set to the transport's workspace root so only
// sessions for this workspace are returned.
func (t *Transport) ListSessions(ctx context.Context) ([]acp.SessionInfo, error) {
	cwd := t.cwd
	req := acp.ListSessionsRequest{
		Cwd: &cwd,
	}
	if err := req.Validate(); err != nil {
		return nil, fmt.Errorf("validate list sessions request: %w", err)
	}
	resp, err := t.conn.ListSessions(ctx, req)
	if err != nil {
		return nil, err
	}
	return resp.Sessions, nil
}

// Prompt sends a user prompt containing content to the given ACP session.
// Structured context resources are translated to ACP content blocks based on
// the agent's advertised prompt capabilities: when the agent supports
// embeddedContext, each resource is sent as an inline ResourceBlock (uri,
// mimeType, text); otherwise it is sent as a ResourceLinkBlock (always
// supported per spec) plus a TextBlock folding the resource text in, so
// non-embeddedContext agents still see the content. Attachments are translated
// as before based on the Image capability. File read errors fall back to the
// resource-link path rather than failing the whole prompt. Returns the agent's
// StopReason (added for ACP spec item 1.2 readiness).
func (t *Transport) Prompt(ctx context.Context, sessionID, content string, resources []ContextResource, attachments []interfaces.Attachment) (acp.StopReason, error) {
	blocks := buildPromptBlocks(t.promptCaps, content, resources, attachments)

	req := acp.PromptRequest{
		SessionId: acp.SessionId(sessionID),
		Prompt:    blocks,
	}
	if err := req.Validate(); err != nil {
		return "", fmt.Errorf("validate prompt request: %w", err)
	}
	resp, err := t.conn.Prompt(ctx, req)
	if err != nil {
		return "", err
	}
	return resp.StopReason, nil
}

// buildPromptBlocks constructs the ordered []acp.ContentBlock payload for a
// prompt turn. It is extracted from Transport.Prompt so the capability-gated
// block selection (embeddedContext vs. fallback, image vs. resource link) can
// be unit-tested without a live ACP connection.
//
// Block order: [user text] [resources...] [attachments...]. Each resource
// becomes either one ResourceBlock (embeddedContext) or a ResourceLinkBlock +
// TextBlock pair (fallback). Each attachment becomes either one ImageBlock
// (image capability, file readable) or a ResourceLinkBlock + TextBlock pair.
func buildPromptBlocks(caps acp.PromptCapabilities, content string, resources []ContextResource, attachments []interfaces.Attachment) []acp.ContentBlock {
	blocks := make([]acp.ContentBlock, 0, 1+len(resources)+len(attachments)*2)
	blocks = append(blocks, acp.TextBlock(content))

	for _, r := range resources {
		// Safety net: strip any null bytes from resource text. Binary
		// content would cause "embedded null byte" errors in JSON-RPC.
		text := strings.ReplaceAll(r.Text, "\x00", "")
		if caps.EmbeddedContext {
			blocks = append(blocks, acp.ResourceBlock(acp.EmbeddedResourceResource{
				TextResourceContents: &acp.TextResourceContents{
					Uri:      r.URI,
					MimeType: acp.Ptr(r.MimeType),
					Text:     text,
				},
			}))
		} else {
			// Fallback: resource link (always supported) + fold text into a text block.
			blocks = append(blocks, acp.ResourceLinkBlock(r.Name, r.URI))
			blocks = append(blocks, acp.TextBlock(text))
		}
	}

	for _, att := range attachments {
		if caps.Image {
			data, err := os.ReadFile(att.Path)
			if err != nil {
				// Fall back to resource link + text hint for this attachment.
				blocks = append(blocks, acp.ResourceLinkBlock(att.Name, att.URI))
				blocks = append(blocks, acp.TextBlock(
					fmt.Sprintf("[Attached image: %s at %s — please read this file to view it]", att.Name, att.URI)))
				continue
			}
			uri := att.URI
			blocks = append(blocks, acp.ContentBlock{Image: &acp.ContentBlockImage{
				Data:     base64.StdEncoding.EncodeToString(data),
				MimeType: att.MimeType,
				Type:     "image",
				Uri:      &uri,
			}})
		} else {
			blocks = append(blocks, acp.ResourceLinkBlock(att.Name, att.URI))
			blocks = append(blocks, acp.TextBlock(
				fmt.Sprintf("[Attached image: %s at %s — please read this file to view it]", att.Name, att.URI)))
		}
	}

	return blocks
}

// Cancel sends a cancel notification to the given ACP session, requesting the
// agent to abort any in-progress work.
func (t *Transport) Cancel(ctx context.Context, sessionID string) error {
	req := acp.CancelNotification{
		SessionId: acp.SessionId(sessionID),
	}
	if err := req.Validate(); err != nil {
		return fmt.Errorf("validate cancel notification: %w", err)
	}
	return t.conn.Cancel(ctx, req)
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
