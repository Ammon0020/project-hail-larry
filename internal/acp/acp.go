// Package acp implements the ACP client layer for communicating with AI agents.
// Blueprint references: Sec 6 (ACP Client Layer), Sec 7 (ACP Integration),
// Sec 9 (Agent Lifecycle), Sec 10 (Session Lifecycle).
//
// This layer handles protocol mechanics: process launch, session management,
// prompts, streaming, permissions, cancellation, and event translation.
// It does NOT contain provider-specific code — all agent communication goes
// through the Agent Client Protocol (via coder/acp-go-sdk).
package acp

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"sort"
	"sync"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
	"github.com/coder/acp-go-sdk"
)

// Client implements interfaces.ACPClient.
// It manages agent registration, session lifecycle, and delegates to the
// ACP transport (transport.go) which uses coder/acp-go-sdk for real agent communication.
type Client struct {
	mu           sync.Mutex
	agents       map[string]AgentInfo
	sessions     map[string]*Session
	callbacks    interfaces.ACPCallbacks
	workspaceMgr interfaces.WorkspaceManager
	permMgr      interfaces.PermissionManager
	storePath    string // file path for persisted conversation metadata
	pipeline     *PromptPipeline
}

// AgentInfo describes a registered agent harness.
type AgentInfo struct {
	ID      string       `json:"id"`
	Name    string       `json:"name"`
	Command string       `json:"command"` // launch command (e.g., "claude", "codex")
	Args    []string     `json:"args,omitempty"`
	Models  []AgentModel `json:"models"`
	Warning string       `json:"warning,omitempty"`
}

// AgentModel describes a model offered by an agent.
type AgentModel struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

// Session represents a conversation with an agent. It persists across daemon
// restarts (metadata only); the live ACP transport is (re)started lazily.
type Session struct {
	ID           string    `json:"id"`
	Name         string    `json:"name"`
	AgentID      string    `json:"agentId"`
	ModelID      string    `json:"modelId"`
	Workspace    string    `json:"workspace"`
	Status       string    `json:"status"`
	CreatedAt    time.Time `json:"createdAt"`
	UpdatedAt    time.Time `json:"updatedAt"`
	transport    transportLike
	ACPSessionID string `json:"acpSessionId,omitempty"`
}

// transportLike is the subset of *Transport methods the Client invokes after a
// transport has been started. Defining it as an interface lets tests inject a
// mock transport without spawning a real agent process. *Transport satisfies it.
type transportLike interface {
	NewSession(ctx context.Context, cwd string) (string, error)
	LoadSession(ctx context.Context, acpSessionID string) (string, error)
	DeleteSession(ctx context.Context, acpSessionID string) error
	Prompt(ctx context.Context, sessionID, content string) error
	Cancel(ctx context.Context, sessionID string) error
	Close() error
	StderrTail() string
}

// defaultConversationName is the placeholder name used until the first prompt
// (or an explicit rename) gives the conversation a real title.
const defaultConversationName = "New chat"

// NewClient creates a new ACP client with no registered agents.
func NewClient(workspaceMgr interfaces.WorkspaceManager, permMgr interfaces.PermissionManager) *Client {
	return &Client{
		agents:       make(map[string]AgentInfo),
		sessions:     make(map[string]*Session),
		workspaceMgr: workspaceMgr,
		permMgr:      permMgr,
	}
}

// SetCallbacks registers the callbacks for event notification.
func (c *Client) SetCallbacks(cb interfaces.ACPCallbacks) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.callbacks = cb
}

// SetPipeline installs a prompt middleware pipeline. When set, SendPrompt runs
// the pipeline before each prompt and prepends any injected context to the
// prompt content sent to the agent (and to the PromptSubmitted event). When
// nil, SendPrompt behaves as before (backward compatible).
func (c *Client) SetPipeline(p *PromptPipeline) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.pipeline = p
}

// RegisterAgent adds an agent to the registry.
func (c *Client) RegisterAgent(agent AgentInfo) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.agents[agent.ID] = agent
}

// RemoveAgent removes an agent from the registry.
func (c *Client) RemoveAgent(id string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	delete(c.agents, id)
}

// ListAgents returns registered agent harnesses and their models.
func (c *Client) ListAgents(_ context.Context) ([]interfaces.AgentInfo, error) {
	c.mu.Lock()
	defer c.mu.Unlock()

	agents := make([]interfaces.AgentInfo, 0, len(c.agents))
	for _, a := range c.agents {
		models := make([]interfaces.AgentModel, 0, len(a.Models))
		for _, m := range a.Models {
			models = append(models, interfaces.AgentModel{
				ID:   m.ID,
				Name: m.Name,
			})
		}
		agents = append(agents, interfaces.AgentInfo{
			ID:      a.ID,
			Name:    a.Name,
			Models:  models,
			Warning: a.Warning,
		})
	}
	return agents, nil
}

// CreateSession starts a new agent session.
// Spawns the agent process, performs ACP handshake (Initialize + NewSession),
// and stores the transport for subsequent prompt/cancel calls.
func (c *Client) CreateSession(ctx context.Context, agentID, modelID, workspaceID string) (interfaces.SessionInfo, error) {
	c.mu.Lock()
	defer c.mu.Unlock()

	// Verify the agent exists.
	agent, ok := c.agents[agentID]
	if !ok {
		return interfaces.SessionInfo{}, fmt.Errorf("agent not found: %s", agentID)
	}

	// Verify the model is offered by the agent.
	modelValid := false
	for _, m := range agent.Models {
		if m.ID == modelID {
			modelValid = true
			break
		}
	}
	if !modelValid {
		return interfaces.SessionInfo{}, fmt.Errorf("model %s not available for agent %s", modelID, agentID)
	}

	sessionID, err := generateSessionID()
	if err != nil {
		return interfaces.SessionInfo{}, fmt.Errorf("generate session ID: %w", err)
	}

	now := time.Now().UTC()
	session := &Session{
		ID:        sessionID,
		Name:      defaultConversationName,
		AgentID:   agentID,
		ModelID:   modelID,
		Workspace: workspaceID,
		Status:    "created",
		CreatedAt: now,
		UpdatedAt: now,
	}

	if err := c.startTransportLocked(ctx, session); err != nil {
		return interfaces.SessionInfo{}, err
	}

	c.sessions[sessionID] = session
	c.persistLocked()

	// Note: no event emitted here — session creation is not a prompt.
	// The UI learns about the session via the ListSessions API.

	return sessionToInfo(session), nil
}

// resolveWorkspacePath returns the on-disk path for a workspace ID by looking
// it up via the workspace manager. It returns the workspaceID unchanged when
// the manager is nil or the workspace is not found (matching the fallback
// behavior of startTransportLocked).
func (c *Client) resolveWorkspacePath(ctx context.Context, workspaceID string) string {
	if c.workspaceMgr == nil {
		return workspaceID
	}
	wlist, err := c.workspaceMgr.List(ctx)
	if err != nil {
		return workspaceID
	}
	for _, w := range wlist {
		if w.ID == workspaceID {
			return w.Path
		}
	}
	return workspaceID
}

// startTransportLocked spawns the agent process for a session and performs the
// ACP handshake (Initialize + NewSession). The caller must hold c.mu. It is used
// both on initial creation and to lazily (re)start a session loaded from disk or
// rebound to a different agent/model.
func (c *Client) startTransportLocked(ctx context.Context, session *Session) error {
	agent, ok := c.agents[session.AgentID]
	if !ok {
		return fmt.Errorf("agent not found: %s", session.AgentID)
	}

	// Determine workspace path for the agent process.
	workspacePath := session.Workspace
	if c.workspaceMgr != nil {
		if wlist, err := c.workspaceMgr.List(ctx); err == nil {
			for _, w := range wlist {
				if w.ID == session.Workspace {
					workspacePath = w.Path
					break
				}
			}
		}
	}

	transport := NewTransport()
	impl := &acpClientImpl{
		callbacks:     c.callbacks,
		workspaceMgr:  c.workspaceMgr,
		permMgr:       c.permMgr,
		workspaceID:   session.Workspace,
		workspacePath: workspacePath,
		sessionID:     session.ID,
		terminals:     make(map[string]*terminalEntry),
	}

	if err := transport.Start(context.Background(), agent.Command, agent.Args, workspacePath, impl); err != nil {
		return fmt.Errorf("start transport: %w", err)
	}
	initResp, err := transport.Initialize(ctx)
	if err != nil {
		_ = transport.Close()
		return fmt.Errorf("initialize transport: %w", err)
	}

	// Decide whether to resume a persisted ACP session via session/load or
	// create a fresh one. LoadSession is only attempted when the agent
	// advertised the loadSession capability AND we have a persisted ACP session
	// ID. On any failure (session gone, capability unsupported, transport error)
	// we fall back to NewSession and overwrite the persisted ID.
	acpSessionID, err := c.resolveACPSession(ctx, transport, initResp, session, workspacePath)
	if err != nil {
		_ = transport.Close()
		return fmt.Errorf("new acp session: %w", err)
	}

	session.transport = transport
	session.ACPSessionID = acpSessionID
	return nil
}

// resolveACPSession decides whether to resume a persisted ACP session via
// session/load or create a fresh one with session/new. It returns the ACP
// session ID to use (and mutates nothing — the caller assigns it). When
// session.ACPSessionID is non-empty and the agent advertised loadSession, it
// tries LoadSession first; on success the prior session is reused. On any
// failure it falls back to NewSession. The caller must hold c.mu (it reads
// session fields); the transport methods are called under the lock to keep the
// load/new decision atomic with the assignment in startTransportLocked.
func (c *Client) resolveACPSession(ctx context.Context, tr transportLike, initResp acp.InitializeResponse, session *Session, workspacePath string) (string, error) {
	if session.ACPSessionID != "" && initResp.AgentCapabilities.LoadSession {
		if loadedID, loadErr := tr.LoadSession(ctx, session.ACPSessionID); loadErr == nil {
			return loadedID, nil
		}
		// Fall through to NewSession on any load error.
	}
	return tr.NewSession(ctx, workspacePath)
}

// SendPrompt sends a user prompt to the agent and streams responses.
// Emits a PromptSubmitted event, then calls transport.Prompt in a goroutine.
// Response chunks arrive asynchronously via acpClientImpl.SessionUpdate.
func (c *Client) SendPrompt(ctx context.Context, sessionID, content string) error {
	c.mu.Lock()
	session, ok := c.sessions[sessionID]
	if !ok {
		c.mu.Unlock()
		return fmt.Errorf("session not found: %s", sessionID)
	}

	// Lazily (re)start the agent process — sessions loaded from disk or rebound
	// to another model start without a live transport.
	if session.transport == nil {
		if err := c.startTransportLocked(ctx, session); err != nil {
			c.mu.Unlock()
			return fmt.Errorf("start session %s: %w", sessionID, err)
		}
	}

	session.Status = "running"
	session.UpdatedAt = time.Now().UTC()
	// Auto-title the conversation from the first user prompt.
	if session.Name == "" || session.Name == defaultConversationName {
		session.Name = titleFromPrompt(content)
	}
	c.persistLocked()
	callbacks := c.callbacks
	pipeline := c.pipeline
	c.mu.Unlock()

	// Run the pre-prompt middleware pipeline. If it injects context, prepend it
	// to the content used for both the PromptSubmitted event and the transport
	// call so the UI and the agent see the same prompt. The pipeline tracks the
	// per-session prompt counter internally (bumped on every RunBeforePrompt).
	finalContent := content
	if pipeline != nil {
		workspacePath := c.resolveWorkspacePath(ctx, session.Workspace)
		pc := &PromptContext{
			SessionID:     sessionID,
			WorkspaceID:   session.Workspace,
			WorkspacePath: workspacePath,
			UserPrompt:    content,
		}
		if action, injected := pipeline.RunBeforePrompt(ctx, pc); action == ActionInject && injected != "" {
			finalContent = injected + "\n\n---\n\n" + content
		}
	}

	if callbacks != nil {
		callbacks.OnEvent(interfaces.Event{
			Type:      interfaces.EventPromptSubmitted,
			SessionID: sessionID,
			Timestamp: time.Now().UTC(),
			Role:      "user",
			Content:   finalContent,
		})
	}

	promptCtx := context.WithoutCancel(ctx)
	go func() {
		if callbacks != nil {
			callbacks.OnEvent(interfaces.Event{
				Type:      interfaces.EventResponseStarted,
				SessionID: sessionID,
				Timestamp: time.Now().UTC(),
				Content:   "Agent is thinking…",
			})
		}

		if err := session.transport.Prompt(promptCtx, session.ACPSessionID, finalContent); err != nil {
			c.mu.Lock()
			session.Status = "failed"
			// The transport is likely dead; drop it so the next prompt restarts.
			tail := ""
			if session.transport != nil {
				tail = session.transport.StderrTail()
			}
			session.transport = nil
			session.ACPSessionID = ""
			c.mu.Unlock()

			summary := err.Error()
			if tail != "" {
				summary = summary + "\n" + tail
			}
			if callbacks != nil {
				callbacks.OnEvent(interfaces.Event{
					Type:      interfaces.EventAgentExited,
					SessionID: sessionID,
					Timestamp: time.Now().UTC(),
					Summary:   summary,
				})
			}
			return
		}

		// Prompt completed successfully — emit a final empty StreamUpdate
		// with streaming=false so the frontend knows the response is complete
		// and removes the typing cursor.
		if callbacks != nil {
			callbacks.OnEvent(interfaces.Event{
				Type:      interfaces.EventStreamUpdate,
				SessionID: sessionID,
				Timestamp: time.Now().UTC(),
				Role:      "agent",
				Content:   "",
				Streaming: false,
			})
		}

		c.mu.Lock()
		if session.Status == "running" {
			session.Status = "completed"
		}
		c.mu.Unlock()
	}()

	return nil
}

// CancelSession interrupts a running session.
func (c *Client) CancelSession(ctx context.Context, sessionID string) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	session, ok := c.sessions[sessionID]
	if !ok {
		return fmt.Errorf("session not found: %s", sessionID)
	}

	session.Status = "interrupted"
	session.UpdatedAt = time.Now().UTC()

	// Send an ACP cancel notification to stop the current turn but keep the
	// agent process alive so the conversation can continue.
	if session.transport != nil {
		_ = session.transport.Cancel(ctx, session.ACPSessionID)
	}
	c.persistLocked()

	// Emit cancellation event.
	if c.callbacks != nil {
		c.callbacks.OnEvent(interfaces.Event{
			Type:      interfaces.EventSessionCancelled,
			SessionID: sessionID,
			Timestamp: time.Now().UTC(),
		})
	}

	return nil
}

// CloseSession terminates the live agent process for a session and removes the
// conversation record. Event history is retained in the event store. Before
// killing the process it makes a best-effort ACP session/delete call (ignored
// on error — the agent may not support session/delete or may already be dead).
func (c *Client) CloseSession(ctx context.Context, sessionID string) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	session, ok := c.sessions[sessionID]
	if !ok {
		return fmt.Errorf("session not found: %s", sessionID)
	}

	if session.transport != nil {
		// Best-effort ACP session/delete before killing the process. Errors are
		// ignored: the agent may not advertise session/delete (unstable) or the
		// subprocess may already have exited.
		if session.ACPSessionID != "" {
			_ = session.transport.DeleteSession(ctx, session.ACPSessionID)
		}
		_ = session.transport.Close()
	}

	// Drop cached permission policies for this session so allow_always /
	// allow_session decisions do not leak into future sessions reusing the ID.
	if c.permMgr != nil {
		c.permMgr.ClearSession(session.ID)
	}

	delete(c.sessions, sessionID)
	c.persistLocked()

	return nil
}

// closeTransportLocked closes the live ACP transport for a session without
// removing the session metadata from c.sessions. It performs a best-effort ACP
// session/delete (ignored on error — the agent may not support session/delete
// or may already be dead), closes the transport, clears cached permission
// policies, and marks the session idle with no live transport. The caller must
// hold c.mu and is responsible for calling persistLocked once after sweeping
// all sessions (so a shutdown writes a single file rather than one per
// session). This is used by CloseAllSessions on daemon shutdown so conversation
// metadata survives a restart; user-initiated deletion still uses CloseSession,
// which removes the record.
func (c *Client) closeTransportLocked(ctx context.Context, sessionID string) error {
	session, ok := c.sessions[sessionID]
	if !ok {
		return fmt.Errorf("session not found: %s", sessionID)
	}

	if session.transport != nil {
		// Best-effort ACP session/delete before killing the process. Errors are
		// ignored: the agent may not advertise session/delete (unstable) or the
		// subprocess may already have exited.
		if session.ACPSessionID != "" {
			_ = session.transport.DeleteSession(ctx, session.ACPSessionID)
		}
		_ = session.transport.Close()
		session.transport = nil
	}

	// Drop cached permission policies for this session so allow_always /
	// allow_session decisions do not leak into future sessions reusing the ID.
	if c.permMgr != nil {
		c.permMgr.ClearSession(session.ID)
	}

	// Mark idle so the UI does not show the session as "running" after restart.
	// LoadConversations also resets status to "idle", but setting it here keeps
	// the on-disk file consistent in case the daemon is inspected post-shutdown.
	session.Status = "idle"
	session.UpdatedAt = time.Now().UTC()
	return nil
}

// CloseAllSessions gracefully closes every active session's live transport,
// calling ACP session/delete (best-effort) and terminating each agent process.
// Unlike CloseSession, it preserves session metadata in c.sessions so
// conversations survive a daemon restart — only the live transport and
// permission policies are torn down. It is intended for daemon shutdown so
// SIGINT/SIGTERM triggers graceful close instead of killing processes outright.
// The session map is swept under a single lock and persisted once at the end.
// The last non-nil error is returned (individual session failures do not abort
// the sweep).
func (c *Client) CloseAllSessions(ctx context.Context) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	var lastErr error
	for id := range c.sessions {
		if err := c.closeTransportLocked(ctx, id); err != nil {
			lastErr = err
		}
	}
	// Persist once after all transports are closed so the on-disk file reflects
	// the surviving metadata (status idle, no live transport).
	c.persistLocked()
	return lastErr
}

// RenameSession changes a conversation's display name.
func (c *Client) RenameSession(sessionID, name string) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	session, ok := c.sessions[sessionID]
	if !ok {
		return fmt.Errorf("session not found: %s", sessionID)
	}
	session.Name = name
	session.UpdatedAt = time.Now().UTC()
	c.persistLocked()
	return nil
}

// RebindSession switches a conversation to a different agent and/or model while
// preserving its id and event history. The live ACP session is closed; a fresh
// one starts on the next prompt (the new agent does not inherit prior context).
func (c *Client) RebindSession(_ context.Context, sessionID, agentID, modelID string) (interfaces.SessionInfo, error) {
	c.mu.Lock()
	defer c.mu.Unlock()

	session, ok := c.sessions[sessionID]
	if !ok {
		return interfaces.SessionInfo{}, fmt.Errorf("session not found: %s", sessionID)
	}

	agent, ok := c.agents[agentID]
	if !ok {
		return interfaces.SessionInfo{}, fmt.Errorf("agent not found: %s", agentID)
	}
	modelValid := false
	for _, m := range agent.Models {
		if m.ID == modelID {
			modelValid = true
			break
		}
	}
	if !modelValid {
		return interfaces.SessionInfo{}, fmt.Errorf("model %s not available for agent %s", modelID, agentID)
	}

	// Tear down the old transport; it restarts lazily on the next prompt.
	if session.transport != nil {
		_ = session.transport.Close()
		session.transport = nil
		session.ACPSessionID = ""
	}

	session.AgentID = agentID
	session.ModelID = modelID
	session.Status = "idle"
	session.UpdatedAt = time.Now().UTC()
	c.persistLocked()

	if c.callbacks != nil {
		c.callbacks.OnEvent(interfaces.Event{
			Type:      interfaces.EventConnectionRestarted,
			SessionID: sessionID,
			Timestamp: time.Now().UTC(),
			Content:   fmt.Sprintf("Switched to %s / %s — prior history kept; the agent's memory restarts.", agent.Name, modelID),
		})
	}

	return sessionToInfo(session), nil
}

// GetSession returns session info by ID.
func (c *Client) GetSession(sessionID string) (*Session, error) {
	c.mu.Lock()
	defer c.mu.Unlock()

	session, ok := c.sessions[sessionID]
	if !ok {
		return nil, fmt.Errorf("session not found: %s", sessionID)
	}
	return session, nil
}

// GetSessionInfo returns the interface-layer projection of a single session by
// ID. It satisfies interfaces.ACPClient so the server package can fetch one
// session without depending on the concrete acp.Session type.
func (c *Client) GetSessionInfo(sessionID string) (interfaces.SessionInfo, error) {
	session, err := c.GetSession(sessionID)
	if err != nil {
		return interfaces.SessionInfo{}, err
	}
	return sessionToInfo(session), nil
}

// sessionToInfo projects a concrete *Session into the interface-layer
// SessionInfo struct. It is used by GetSessionInfo and the list/create/rebind
// paths so every caller returns a consistent shape.
func sessionToInfo(s *Session) interfaces.SessionInfo {
	return interfaces.SessionInfo{
		ID:        s.ID,
		Name:      s.Name,
		Status:    s.Status,
		AgentID:   s.AgentID,
		ModelID:   s.ModelID,
		Workspace: s.Workspace,
		CreatedAt: s.CreatedAt,
		UpdatedAt: s.UpdatedAt,
	}
}

// ListSessions returns all conversations, newest activity first.
func (c *Client) ListSessions() []Session {
	c.mu.Lock()
	defer c.mu.Unlock()

	sessions := make([]Session, 0, len(c.sessions))
	for _, s := range c.sessions {
		sessions = append(sessions, *s)
	}
	sort.Slice(sessions, func(i, j int) bool {
		return sessions[i].UpdatedAt.After(sessions[j].UpdatedAt)
	})
	return sessions
}

// generateSessionID generates a unique session ID using crypto/rand.
func generateSessionID() (string, error) {
	b := make([]byte, 8)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return "sess-" + hex.EncodeToString(b), nil
}
