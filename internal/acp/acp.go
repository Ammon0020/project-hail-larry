// Package acp implements the ACP client layer for communicating with AI agents.
// Blueprint references: Sec 6 (ACP Client Layer), Sec 7 (ACP Integration),
// Sec 9 (Agent Lifecycle), Sec 10 (Session Lifecycle).
//
// This layer handles protocol mechanics: process launch, session management,
// prompts, streaming, permissions, cancellation, and event translation.
// It does NOT contain provider-specific code — all agent communication goes
// through ACP (stdio JSON-RPC).
package acp

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"sync"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
)

// Client implements interfaces.ACPClient.
// It manages agent registration, session lifecycle, and delegates to the
// ACP stdio JSON-RPC transport (transport.go) for real agent communication.
type Client struct {
	mu           sync.Mutex
	agents       map[string]AgentInfo
	sessions     map[string]*Session
	callbacks    interfaces.ACPCallbacks
	workspaceMgr interfaces.WorkspaceManager
	permMgr      interfaces.PermissionManager
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

// Session represents an active agent session.
type Session struct {
	ID           string    `json:"id"`
	AgentID      string    `json:"agentId"`
	ModelID      string    `json:"modelId"`
	Workspace    string    `json:"workspace"`
	Status       string    `json:"status"`
	CreatedAt    time.Time `json:"createdAt"`
	transport    *Transport
	acpSessionID string
}

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

	// Determine workspace path for the agent process
	workspacePath := workspaceID
	if c.workspaceMgr != nil {
		wlist, err := c.workspaceMgr.List(ctx)
		if err == nil {
			for _, w := range wlist {
				if w.ID == workspaceID {
					workspacePath = w.Path
					break
				}
			}
		}
	}

	transport := NewTransport()
	impl := &acpClientImpl{
		callbacks:    c.callbacks,
		workspaceMgr: c.workspaceMgr,
		permMgr:      c.permMgr,
		workspaceID:  workspaceID,
		sessionID:    sessionID,
	}

	if err := transport.Start(ctx, agent.Command, agent.Args, workspacePath, impl); err != nil {
		return interfaces.SessionInfo{}, fmt.Errorf("start transport: %w", err)
	}

	if _, err := transport.Initialize(ctx); err != nil {
		_ = transport.Close()
		return interfaces.SessionInfo{}, fmt.Errorf("initialize transport: %w", err)
	}

	acpSessionID, err := transport.NewSession(ctx, workspacePath)
	if err != nil {
		_ = transport.Close()
		return interfaces.SessionInfo{}, fmt.Errorf("new acp session: %w", err)
	}

	session := &Session{
		ID:           sessionID,
		AgentID:      agentID,
		ModelID:      modelID,
		Workspace:    workspaceID,
		Status:       "created",
		CreatedAt:    time.Now().UTC(),
		transport:    transport,
		acpSessionID: acpSessionID,
	}

	c.sessions[sessionID] = session

	// Note: no event emitted here — session creation is not a prompt.
	// The UI learns about the session via the ListSessions API.

	return interfaces.SessionInfo{
		ID:     sessionID,
		Name:   fmt.Sprintf("Session %s", sessionID[:8]),
		Status: session.Status,
	}, nil
}

// SendPrompt sends a user prompt to the agent and streams responses.
// Emits a PromptSubmitted event, then calls transport.Prompt in a goroutine.
// Response chunks arrive asynchronously via acpClientImpl.SessionUpdate.
func (c *Client) SendPrompt(ctx context.Context, sessionID, content string) error {
	c.mu.Lock()
	session, ok := c.sessions[sessionID]
	if ok {
		session.Status = "running"
	}
	callbacks := c.callbacks
	c.mu.Unlock()

	if !ok {
		return fmt.Errorf("session not found: %s", sessionID)
	}
	if session.transport == nil {
		return fmt.Errorf("session transport not initialized: %s", sessionID)
	}

	if callbacks != nil {
		callbacks.OnEvent(interfaces.Event{
			Type:      interfaces.EventPromptSubmitted,
			SessionID: sessionID,
			Timestamp: time.Now().UTC(),
			Role:      "user",
			Content:   content,
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

		if err := session.transport.Prompt(promptCtx, session.acpSessionID, content); err != nil {
			c.mu.Lock()
			session.Status = "failed"
			c.mu.Unlock()
			if callbacks != nil {
				callbacks.OnEvent(interfaces.Event{
					Type:      interfaces.EventAgentExited,
					SessionID: sessionID,
					Timestamp: time.Now().UTC(),
					Summary:   err.Error(),
				})
			}
			return
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

	if session.transport != nil {
		_ = session.transport.Cancel(ctx, session.acpSessionID)
		_ = session.transport.Close()
	}

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

// CloseSession closes a session.
func (c *Client) CloseSession(ctx context.Context, sessionID string) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	session, ok := c.sessions[sessionID]
	if !ok {
		return fmt.Errorf("session not found: %s", sessionID)
	}

	if session.transport != nil {
		_ = session.transport.Close()
	}

	session.Status = "completed"
	delete(c.sessions, sessionID)

	return nil
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

// ListSessions returns all active sessions.
func (c *Client) ListSessions() []Session {
	c.mu.Lock()
	defer c.mu.Unlock()

	sessions := make([]Session, 0, len(c.sessions))
	for _, s := range c.sessions {
		sessions = append(sessions, *s)
	}
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
