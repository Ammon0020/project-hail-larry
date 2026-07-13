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
	"errors"
	"fmt"
	"log/slog"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
	"github.com/adama/local-agent/internal/mcp"
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
	// eventStore is used by RebindSession to export the prior conversation
	// history before switching to a new agent. Optional; when nil the
	// conversation transfer is skipped.
	eventStore interfaces.EventStore
	// transfer is the middleware that queues exported transcripts for injection
	// into the first prompt of a rebound session. Optional; when nil
	// RebindSession skips the export/queue step.
	transfer *ConversationTransferMiddleware
	// mcpConfigPath is the path to mcp.json (typically
	// ~/.local-agent/mcp.json). When set, startTransportLocked loads it after
	// Initialize and passes the enabled, capability-filtered server list to
	// the agent on session/new and session/load. When empty, no MCP servers are
	// passed (the pre-MCP-config behavior).
	mcpConfigPath string
	// profileMiddleware holds the profile mode middleware so the REST handler
	// can call SetProfile before each prompt. Nil until wired by the daemon.
	profileMiddleware *ProfileMiddleware
}

// ClientConfig contains the dependencies and optional settings for an ACP client.
type ClientConfig struct {
	WorkspaceMgr  interfaces.WorkspaceManager
	PermissionMgr interfaces.PermissionManager
	Pipeline      *PromptPipeline
	EventStore    interfaces.EventStore
	Transfer      *ConversationTransferMiddleware
	McpConfigPath string
	StorePath     string
	Callbacks     interfaces.ACPCallbacks
}

// AgentInfo describes a registered agent harness.
type AgentInfo = interfaces.AgentInfo

// AgentModel describes a model offered by an agent.
type AgentModel = interfaces.AgentModel

// Session lifecycle statuses. These string values are part of the persisted
// JSON shape (Session.Status) and the wire format sent to the UI, so they must
// remain stable.
const (
	statusCreated     = "created"
	statusRunning     = "running"
	statusIdle        = "idle"
	statusFailed      = "failed"
	statusCompleted   = "completed"
	statusInterrupted = "interrupted"
)

// Role values for chat messages and events.
const (
	roleUser  = "user"
	roleAgent = "agent"
)

// modelConfigCategory is the ACP config-option category used to select a model.
const modelConfigCategory = "model"

// osWindows is the runtime.GOOS value for Windows, used by platform branches.
const osWindows = "windows"

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
	// modelConfigID is the config option ID for the model selector (category
	// "model"), captured from the agent's NewSession/LoadSession response.
	// Empty when the agent doesn't advertise a model config option. Used by
	// SwitchModel to call session/set_config_option without restart. Not
	// persisted: it is re-derived from the agent's response every time the
	// transport (re)starts.
	modelConfigID string
	// impl holds the acpClientImpl passed to the transport for this session.
	// It is retained so that CloseSession / closeTransportLocked can cancel
	// outstanding terminal subprocesses and their goroutines before the
	// transport is torn down, preventing leaks on session close and shutdown.
	impl *acpClientImpl
}

// transportLike is the subset of *Transport methods the Client invokes after a
// transport has been started. Defining it as an interface lets tests inject a
// mock transport without spawning a real agent process. *Transport satisfies it.
type transportLike interface {
	NewSession(ctx context.Context, cwd string, additionalDirs []string) (string, []acp.SessionConfigOption, error)
	LoadSession(ctx context.Context, acpSessionID string, additionalDirs []string) (string, []acp.SessionConfigOption, error)
	ListSessions(ctx context.Context) ([]acp.SessionInfo, error)
	DeleteSession(ctx context.Context, acpSessionID string) error
	Prompt(ctx context.Context, sessionID, content string, resources []ContextResource, attachments []interfaces.Attachment) (acp.StopReason, error)
	Cancel(ctx context.Context, sessionID string) error
	Close() error
	StderrTail() string
	SetSessionConfigOption(ctx context.Context, sessionID, configID, value string) error
	// SupportsEmbeddedContext reports whether the agent advertised the
	// embeddedContext prompt capability during Initialize.
	SupportsEmbeddedContext() bool
	// SupportsProviders reports whether the agent advertised the (unstable)
	// providers capability during Initialize.
	SupportsProviders() bool
	// ListProviders calls the unstable ACP providers/list method.
	ListProviders(ctx context.Context) ([]acp.UnstableProviderInfo, error)
	// SetProvider calls the unstable ACP providers/set method.
	SetProvider(ctx context.Context, id, apiType, baseURL string, headers map[string]any) error
	// DisableProvider calls the unstable ACP providers/disable method.
	DisableProvider(ctx context.Context, id string) error
}

// TransportLike is the exported alias of the internal transportLike interface,
// so tests in other packages (e.g. internal/server) can inject a mock transport
// via Client.SetTransportForTest. *Transport satisfies it.
type TransportLike = transportLike

// ErrProvidersUnsupported is returned by the provider-management Client methods
// when the agent did not advertise the (unstable) providers capability during
// Initialize. Callers should surface this as a clear "not supported by this
// agent" error rather than a generic failure.
var ErrProvidersUnsupported = errors.New("agent does not support the providers capability")

// defaultConversationName is the placeholder name used until the first prompt
// (or an explicit rename) gives the conversation a real title.
const defaultConversationName = "New chat"

// promptTimeout is the generous but finite deadline applied to every prompt
// sent to an agent. The prompt context is detached from the caller's context
// (so it survives the HTTP request that initiated it) but must still be bounded
// — otherwise a hung agent (stuck, deadlocked, or waiting on an unanswered
// permission prompt) blocks the prompt goroutine forever and leaks it. When the
// timeout fires, the SDK's Prompt returns a context-deadline error which the
// existing failure-handling branch converts into an EventAgentExited event and
// resets the transport.
const promptTimeout = 10 * time.Minute

// NewClient creates a new ACP client with no registered agents.
func NewClient(cfg ClientConfig) *Client {
	return &Client{
		agents:        make(map[string]AgentInfo),
		sessions:      make(map[string]*Session),
		callbacks:     cfg.Callbacks,
		workspaceMgr:  cfg.WorkspaceMgr,
		permMgr:       cfg.PermissionMgr,
		storePath:     cfg.StorePath,
		pipeline:      cfg.Pipeline,
		eventStore:    cfg.EventStore,
		transfer:      cfg.Transfer,
		mcpConfigPath: cfg.McpConfigPath,
	}
}

// SetCallbacks registers the callbacks for event notification.
func (c *Client) SetCallbacks(cb interfaces.ACPCallbacks) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.callbacks = cb
}

// emit forwards an event to the registered callbacks, if any. Safe to call
// while holding c.mu (the common case) — it only reads c.callbacks, which is
// mutated under the same lock. Callers that emit outside the lock (e.g.
// SendPrompt's detached goroutine) capture callbacks into a local variable
// first and call it directly rather than via this helper.
func (c *Client) emit(ev interfaces.Event) {
	if c.callbacks != nil {
		c.callbacks.OnEvent(ev)
	}
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

// SetEventStore installs the event store used by RebindSession to export the
// prior conversation history when switching agents. When set, RebindSession
// reads the session's events and queues a markdown transcript for injection
// into the new agent's first prompt. When nil (the default), rebind skips the
// conversation export and the new agent starts fresh.
func (c *Client) SetEventStore(store interfaces.EventStore) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.eventStore = store
}

// SetConversationTransfer installs the middleware that queues exported
// conversation transcripts for injection into the first prompt of a rebound
// session. RebindSession calls SetTransfer on it after exporting the prior
// conversation. When nil, RebindSession skips the transfer-queue step.
func (c *Client) SetConversationTransfer(m *ConversationTransferMiddleware) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.transfer = m
}

// SetMcpConfigPath configures the path to mcp.json. When set, the client loads
// MCP server config from this file at session start (after Initialize returns
// the agent's McpCapabilities) and passes the enabled, capability-filtered
// server list to the agent on session/new and session/load. When empty (the
// default), no MCP servers are passed. The path is typically
// ~/.local-agent/mcp.json and is wired by the daemon.
func (c *Client) SetMcpConfigPath(path string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.mcpConfigPath = path
}

// SetProfileMiddleware installs the profile middleware so the REST handler can
// set per-session profiles via SetSessionProfile. Called by the daemon during
// initialization.
func (c *Client) SetProfileMiddleware(m *ProfileMiddleware) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.profileMiddleware = m
}

// SetSessionProfile sets the user's selected profile (Code/Ask/Plan) for a
// session. The profile middleware reads this before each prompt and injects
// the corresponding system instructions.
func (c *Client) SetSessionProfile(sessionID, profile string) {
	c.mu.Lock()
	pm := c.profileMiddleware
	c.mu.Unlock()
	if pm != nil {
		pm.SetProfile(sessionID, profile)
	}
}

// loadMcpServersLocked reads mcp.json and returns the enabled, capability-
// filtered MCP server list. The caller must hold c.mu (it reads c.mcpConfigPath
// and is invoked from startTransportLocked). A missing file returns an empty
// list with no error; any other read/parse/translation error is returned so
// the caller can log it and continue without MCP servers.
func (c *Client) loadMcpServersLocked(caps acp.McpCapabilities) ([]acp.McpServer, error) {
	f, err := mcp.Load(c.mcpConfigPath)
	if err != nil {
		return nil, fmt.Errorf("load mcp config %q: %w", c.mcpConfigPath, err)
	}
	servers, err := mcp.ToACPSlice(f, caps)
	if err != nil {
		return nil, fmt.Errorf("translate mcp config: %w", err)
	}
	return servers, nil
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

// validateAgentModelLocked verifies that agentID is registered and modelID is
// offered by that agent, returning the resolved AgentInfo. The caller must hold
// c.mu (it reads c.agents). Used by CreateSession and RebindSession to share
// the same agent+model validation and error messages.
func (c *Client) validateAgentModelLocked(agentID, modelID string) (AgentInfo, error) {
	agent, ok := c.agents[agentID]
	if !ok {
		return AgentInfo{}, fmt.Errorf("agent not found: %s", agentID)
	}
	modelValid := false
	for _, m := range agent.Models {
		if m.ID == modelID {
			modelValid = true
			break
		}
	}
	if !modelValid {
		return AgentInfo{}, fmt.Errorf("model %s not available for agent %s", modelID, agentID)
	}
	return agent, nil
}

// CreateSession starts a new agent session.
// Spawns the agent process, performs ACP handshake (Initialize + NewSession),
// and stores the transport for subsequent prompt/cancel calls.
func (c *Client) CreateSession(ctx context.Context, agentID, modelID, workspaceID string) (interfaces.SessionInfo, error) {
	c.mu.Lock()
	defer c.mu.Unlock()

	if _, err := c.validateAgentModelLocked(agentID, modelID); err != nil {
		return interfaces.SessionInfo{}, err
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
		Status:    statusCreated,
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

// collectAdditionalDirsLocked returns the absolute paths of every registered
// workspace except the session's primary workspace, suitable for passing as the
// ACP `additionalDirectories` field on session/new and session/load. The
// primary workspace is excluded both by ID (primaryID) and by path
// (primaryPath, after filepath.Clean) so a workspace registered twice or
// resolved via a different ID is still skipped. Non-absolute paths and
// duplicates of the primary are skipped; the resulting slice has no ordering
// guarantees beyond what workspaceMgr.List provides (currently sorted by name).
//
// The caller must hold c.mu (this reads c.workspaceMgr, which is immutable after
// construction but the lock is held by startTransportLocked for consistency with
// the surrounding workspace-path resolution).
func (c *Client) collectAdditionalDirsLocked(ctx context.Context, primaryID, primaryPath string) []string {
	if c.workspaceMgr == nil {
		return nil
	}
	wlist, err := c.workspaceMgr.List(ctx)
	if err != nil {
		return nil
	}
	primaryClean := filepath.Clean(primaryPath)
	seen := make(map[string]bool, len(wlist))
	out := make([]string, 0, len(wlist))
	for _, w := range wlist {
		if w.ID == primaryID {
			continue
		}
		if !filepath.IsAbs(w.Path) {
			continue
		}
		cleaned := filepath.Clean(w.Path)
		if cleaned == primaryClean {
			continue
		}
		if seen[cleaned] {
			continue
		}
		seen[cleaned] = true
		out = append(out, cleaned)
	}
	return out
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
		// Surface the agent's stderr tail before closing the transport —
		// otherwise the user only sees "peer disconnected before response"
		// with no clue why the agent exited (missing auth, wrong subcommand,
		// crashed on startup, etc.). Close still runs so the process is
		// reaped; we just read the captured tail first.
		stderr := transport.StderrTail()
		_ = transport.Close()
		if strings.TrimSpace(stderr) != "" {
			return fmt.Errorf("initialize transport: %w (agent stderr: %s)", err, strings.TrimSpace(stderr))
		}
		return fmt.Errorf("initialize transport: %w", err)
	}
	// Store the agent's prompt capabilities on the transport so Prompt can
	// build capability-gated content blocks (inline images vs. resource links).
	transport.promptCaps = initResp.AgentCapabilities.PromptCapabilities
	// Capture the (unstable) providers capability so the provider-management
	// methods can gate on it without re-reading the InitializeResponse. A nil
	// pointer means the agent did not advertise the capability.
	transport.providersSupported = initResp.AgentCapabilities.Providers != nil

	// Authenticate before opening a session when the agent requires it.
	// Devin (and similar hosts) advertise authMethods and reject
	// session/new with "ACP host has not authenticated" until the client
	// completes authenticate. Prefer an agent-handled method (browser
	// PKCE, etc.); skip env_var/terminal methods here — those need UI
	// credential collection that is not yet wired.
	if methodID := selectAgentAuthMethod(initResp.AuthMethods); methodID != "" {
		if authErr := transport.Authenticate(ctx, methodID); authErr != nil {
			stderr := transport.StderrTail()
			_ = transport.Close()
			if strings.TrimSpace(stderr) != "" {
				return fmt.Errorf("authenticate agent (%s): %w (agent stderr: %s)", methodID, authErr, strings.TrimSpace(stderr))
			}
			return fmt.Errorf("authenticate agent (%s): %w", methodID, authErr)
		}
	}

	// Load MCP server config and filter it against the agent's advertised
	// McpCapabilities before any session RPC. A missing config file is not an
	// error (no servers configured); a parse error is logged and treated as
	// "no servers" so a malformed mcp.json doesn't block session creation.
	if c.mcpConfigPath != "" {
		servers, mcpErr := c.loadMcpServersLocked(initResp.AgentCapabilities.McpCapabilities)
		if mcpErr != nil {
			// Log and continue with no servers rather than failing session
			// creation — MCP config is additive, not load-bearing for the
			// session itself.
			slog.Warn("loading mcp config; continuing without mcp servers", "path", c.mcpConfigPath, "err", mcpErr)
		}
		transport.SetMcpServers(servers)
	}

	// Decide whether to resume a persisted ACP session via session/load or
	// create a fresh one. LoadSession is only attempted when the agent
	// advertised the loadSession capability AND we have a persisted ACP session
	// ID. On any failure (session gone, capability unsupported, transport error)
	// we fall back to NewSession and overwrite the persisted ID.
	//
	// Build the ACP `additionalDirectories` list from every registered
	// workspace except the session's primary cwd, so multi-root / monorepo
	// agents can see files outside the primary workspace. The list is
	// capability-gated: when the agent did not advertise the
	// additionalDirectories session capability we pass nil (the field is
	// omitempty, so nothing extra is sent on the wire and we avoid sending
	// multi-root hints to agents that won't understand them).
	var additionalDirs []string
	if initResp.AgentCapabilities.SessionCapabilities.AdditionalDirectories != nil {
		additionalDirs = c.collectAdditionalDirsLocked(ctx, session.Workspace, workspacePath)
	}
	acpSessionID, configOpts, err := c.resolveACPSession(ctx, transport, initResp, session, workspacePath, additionalDirs)
	if err != nil {
		_ = transport.Close()
		return fmt.Errorf("new acp session: %w", err)
	}

	session.transport = transport
	session.impl = impl
	session.ACPSessionID = acpSessionID

	session.modelConfigID = findModelConfigID(configOpts, agent.Models)
	if session.modelConfigID == "" {
		slog.Info("findModelConfigID returned empty",
			"agent", session.AgentID, "optsCount", len(configOpts),
			"knownModels", len(agent.Models))
	}

	// Apply the user-selected model to the live session if the agent supports
	// model config options. This fixes a prior bug where the model dropdown
	// only updated metadata — the agent never received the model selection.
	// If the agent's current value already matches, the agent is expected to
	// treat this as a no-op.
	if session.modelConfigID != "" && session.ModelID != "" {
		if err := transport.SetSessionConfigOption(ctx, acpSessionID, session.modelConfigID, session.ModelID); err != nil {
			slog.Warn("applying initial model; agent will use its default", "session", session.ID, "model", session.ModelID, "err", err)
		}
	}
	return nil
}

// resolveACPSession decides whether to resume a persisted ACP session via
// session/load or create a fresh one with session/new. It returns the ACP
// session ID to use (and mutates nothing — the caller assigns it).
//
// Reconciliation flow (when the agent supports session/list):
//  1. Call ListSessions filtered by cwd.
//  2. If our persisted ACPSessionID is in the list, attempt LoadSession.
//  3. If it is NOT in the list, skip the doomed LoadSession and go straight to
//     NewSession — the session is known to be gone on the agent side.
//  4. If ListSessions fails, fall back to the legacy flow (try LoadSession,
//     then NewSession).
//
// Legacy flow (no session/list capability):
//  1. When session.ACPSessionID is non-empty and the agent advertised
//     loadSession, try LoadSession first; on success the prior session is
//     reused.
//  2. On any failure fall back to NewSession.
//
// The caller must hold c.mu (it reads session fields); the transport methods
// are called under the lock to keep the load/new decision atomic with the
// assignment in startTransportLocked. In addition to the session ID, it
// returns the agent's advertised session config options (e.g. the model
// selector) so the caller can capture the model config ID for later
// session/set_config_option calls.
//
// additionalDirs is forwarded verbatim to NewSession/LoadSession as the ACP
// `additionalDirectories` field. The caller is responsible for capability-
// gating the list (pass nil when the agent did not advertise the
// additionalDirectories session capability).
func (c *Client) resolveACPSession(ctx context.Context, tr transportLike, initResp acp.InitializeResponse, session *Session, workspacePath string, additionalDirs []string) (string, []acp.SessionConfigOption, error) {
	persistedID := session.ACPSessionID
	canLoad := initResp.AgentCapabilities.LoadSession
	canList := initResp.AgentCapabilities.SessionCapabilities.List != nil

	// When the agent supports session/list, reconcile first: only attempt
	// LoadSession if the agent confirms the session still exists. This avoids
	// a doomed LoadSession RPC when the session is known to be gone. If
	// ListSessions itself fails, fall through to the legacy try-load-then-new
	// flow so we don't regress on agents with flaky list support.
	if persistedID != "" && canLoad && canList {
		if sessions, err := tr.ListSessions(ctx); err == nil {
			if !sessionExists(sessions, persistedID) {
				return tr.NewSession(ctx, workspacePath, additionalDirs)
			}
			// Session confirmed present — attempt LoadSession below.
		}
		// ListSessions error or session found: fall through to LoadSession.
	}

	if persistedID != "" && canLoad {
		if loadedID, opts, loadErr := tr.LoadSession(ctx, persistedID, additionalDirs); loadErr == nil {
			return loadedID, opts, nil
		}
		// Fall through to NewSession on any load error.
	}
	return tr.NewSession(ctx, workspacePath, additionalDirs)
}

// findModelConfigID scans the agent's advertised session config options for the
// model selector and returns its config ID. Returns empty string if no model
// config option can be identified.
//
// The ACP spec makes `category` OPTIONAL and states clients MUST handle missing
// or unknown categories gracefully. Some agents (e.g. Mistral Vibe) omit
// `category` on their model selector, so we cannot rely on it alone. We match
// in priority order:
//  1. category == "model" (the spec-preferred signal)
//  2. option Id == "model" (common convention)
//  3. option Name contains "model" (case-insensitive)
//  4. the option's CurrentValue or one of its Options values matches a known
//     model ID from the agent registry (strongest signal when category is
//     absent and the id/name are generic)
//
// knownModels is the registered agent's model list; pass nil to skip the
// value-match fallback (used in tests).
func findModelConfigID(opts []acp.SessionConfigOption, knownModels []AgentModel) string {
	// Build a set of known model IDs for the value-match fallback.
	known := make(map[string]struct{}, len(knownModels))
	for _, m := range knownModels {
		known[m.ID] = struct{}{}
	}

	// modelMatcher defines a single priority-ordered match strategy. Each
	// matcher is tried in sequence over all config options; the first option
	// that matches wins. needsKnown indicates the matcher requires the known
	// model-ID set (only the value-match fallback does).
	type modelMatcher struct {
		matches    func(opt acp.SessionConfigOption) bool
		needsKnown bool
	}

	matchers := []modelMatcher{
		// Pass 1: explicit category match (spec-preferred).
		{matches: func(opt acp.SessionConfigOption) bool {
			return opt.Select != nil && opt.Select.Category != nil &&
				*opt.Select.Category == acp.SessionConfigOptionCategoryModel
		}},
		// Pass 2: conventional id "model".
		{matches: func(opt acp.SessionConfigOption) bool {
			return opt.Select != nil && string(opt.Select.Id) == modelConfigCategory
		}},
		// Pass 3: name contains "model" (case-insensitive).
		{matches: func(opt acp.SessionConfigOption) bool {
			return opt.Select != nil && strings.Contains(strings.ToLower(opt.Select.Name), "model")
		}},
		// Pass 4: current value or any option value matches a known model ID.
		{matches: func(opt acp.SessionConfigOption) bool {
			if opt.Select == nil {
				return false
			}
			if _, ok := known[string(opt.Select.CurrentValue)]; ok {
				return true
			}
			if opt.Select.Options.Ungrouped != nil {
				for _, v := range *opt.Select.Options.Ungrouped {
					if _, ok := known[string(v.Value)]; ok {
						return true
					}
				}
			}
			if opt.Select.Options.Grouped != nil {
				for _, g := range *opt.Select.Options.Grouped {
					for _, v := range g.Options {
						if _, ok := known[string(v.Value)]; ok {
							return true
						}
					}
				}
			}
			return false
		}, needsKnown: true},
	}

	for _, matcher := range matchers {
		if matcher.needsKnown && len(known) == 0 {
			continue
		}
		for _, opt := range opts {
			if matcher.matches(opt) {
				return string(opt.Select.Id)
			}
		}
	}
	return ""
}

// sessionExists reports whether the given ACP session ID appears in the
// agent's session list. SessionInfo.SessionId is the ACP session identifier.
func sessionExists(sessions []acp.SessionInfo, acpSessionID string) bool {
	for _, s := range sessions {
		if string(s.SessionId) == acpSessionID {
			return true
		}
	}
	return false
}

// SendPrompt sends a user prompt to the agent and streams responses.
// Emits a PromptSubmitted event, then calls transport.Prompt in a goroutine.
// Response chunks arrive asynchronously via acpClientImpl.SessionUpdate.
func (c *Client) SendPrompt(ctx context.Context, sessionID, content string, attachments []interfaces.Attachment) error {
	c.mu.Lock()
	session, err := c.validatePromptSession(sessionID)
	if err != nil {
		c.mu.Unlock()
		return err
	}

	callbacks, pipeline, err := c.preparePromptSessionLocked(ctx, session, sessionID, content)
	c.mu.Unlock()
	if err != nil {
		return err
	}

	finalContent, resources := c.runPromptPipeline(ctx, pipeline, session, sessionID, content)
	c.emitPromptSubmitted(callbacks, sessionID, finalContent, attachments)
	c.startPromptGoroutine(ctx, callbacks, session, sessionID, finalContent, resources, attachments)

	return nil
}

// validatePromptSession looks up the session that will receive a prompt. The
// caller must hold c.mu.
func (c *Client) validatePromptSession(sessionID string) (*Session, error) {
	session, ok := c.sessions[sessionID]
	if !ok {
		return nil, fmt.Errorf("session not found: %s", sessionID)
	}
	return session, nil
}

// preparePromptSessionLocked ensures the session has a live transport and
// records its running state before the prompt pipeline starts. The caller must
// hold c.mu.
func (c *Client) preparePromptSessionLocked(
	ctx context.Context,
	session *Session,
	sessionID string,
	content string,
) (interfaces.ACPCallbacks, *PromptPipeline, error) {
	// Lazily (re)start the agent process — sessions loaded from disk or rebound
	// to another model start without a live transport.
	if session.transport == nil {
		if err := c.startTransportLocked(ctx, session); err != nil {
			return nil, nil, fmt.Errorf("start session %s: %w", sessionID, err)
		}
	}

	session.Status = statusRunning
	session.UpdatedAt = time.Now().UTC()
	// Auto-title the conversation from the first user prompt.
	if session.Name == "" || session.Name == defaultConversationName {
		session.Name = titleFromPrompt(content)
	}
	c.persistLocked()
	callbacks := c.callbacks
	pipeline := c.pipeline
	return callbacks, pipeline, nil
}

// runPromptPipeline applies the pre-prompt middleware and returns the content
// and structured resources that should be sent to the agent.
func (c *Client) runPromptPipeline(
	ctx context.Context,
	pipeline *PromptPipeline,
	session *Session,
	sessionID string,
	content string,
) (string, []ContextResource) {
	// Run the pre-prompt middleware pipeline. If it injects context, prepend any
	// free-form text to the content used for both the PromptSubmitted event and
	// the transport call so the UI and the agent see the same prompt, and pass
	// the structured resources to the transport so it can render them as ACP
	// resource ContentBlocks (or fold into text for non-embeddedContext agents).
	// The pipeline tracks the per-session prompt counter internally (bumped on
	// every RunBeforePrompt).
	var resources []ContextResource
	finalContent := content
	if pipeline != nil {
		workspacePath := c.resolveWorkspacePath(ctx, session.Workspace)
		// Populate EmbeddedContext from the agent's advertised prompt
		// capabilities so middlewares can dynamically choose between
		// structured resource blocks and text injection.
		embeddedCtx := false
		if session.transport != nil {
			embeddedCtx = session.transport.SupportsEmbeddedContext()
		}
		pc := &PromptContext{
			SessionID:       sessionID,
			WorkspaceID:     session.Workspace,
			WorkspacePath:   workspacePath,
			UserPrompt:      content,
			EmbeddedContext: embeddedCtx,
		}
		if action, res := pipeline.RunBeforePrompt(ctx, pc); action == ActionInject {
			resources = res.Resources
			if res.Text != "" {
				finalContent = res.Text + "\n\n---\n\n" + content
			}
		}
	}
	return finalContent, resources
}

// emitPromptSubmitted emits the user-visible prompt after middleware has
// finished injecting text and structured context.
func (c *Client) emitPromptSubmitted(
	callbacks interfaces.ACPCallbacks,
	sessionID string,
	content string,
	attachments []interfaces.Attachment,
) {
	if callbacks != nil {
		callbacks.OnEvent(interfaces.Event{
			Type:        interfaces.EventPromptSubmitted,
			SessionID:   sessionID,
			Timestamp:   time.Now().UTC(),
			Role:        roleUser,
			Content:     content,
			Attachments: attachments,
		})
	}
}

// startPromptGoroutine starts the bounded asynchronous prompt call and updates
// the session and event stream when the agent completes or fails.
func (c *Client) startPromptGoroutine(
	ctx context.Context,
	callbacks interfaces.ACPCallbacks,
	session *Session,
	sessionID string,
	content string,
	resources []ContextResource,
	attachments []interfaces.Attachment,
) {
	// Detach from the caller's context (the HTTP request ends when this
	// method returns) but apply a finite timeout so a hung agent cannot block
	// the prompt goroutine forever. Without this bound, an agent that stays
	// alive but stops responding to the JSON-RPC session/prompt call would
	// leak the goroutine and leave the session stuck in "running" forever.
	promptCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), promptTimeout)
	go func() {
		defer cancel() // release timer resources when the prompt completes
		if callbacks != nil {
			callbacks.OnEvent(interfaces.Event{
				Type:      interfaces.EventResponseStarted,
				SessionID: sessionID,
				Timestamp: time.Now().UTC(),
				Content:   "Agent is thinking…",
			})
		}

		stopReason, err := session.transport.Prompt(promptCtx, session.ACPSessionID, content, resources, attachments)
		if err != nil {
			c.mu.Lock()
			session.Status = statusFailed
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
				Type:       interfaces.EventStreamUpdate,
				SessionID:  sessionID,
				Timestamp:  time.Now().UTC(),
				Role:       roleAgent,
				Content:    "",
				Streaming:  false,
				StopReason: string(stopReason),
			})
		}

		c.mu.Lock()
		if session.Status == statusRunning {
			session.Status = statusCompleted
		}
		c.mu.Unlock()
	}()
}

// CancelSession interrupts a running session.
func (c *Client) CancelSession(ctx context.Context, sessionID string) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	session, ok := c.sessions[sessionID]
	if !ok {
		return fmt.Errorf("session not found: %s", sessionID)
	}

	session.Status = statusInterrupted
	session.UpdatedAt = time.Now().UTC()

	// Send an ACP cancel notification to stop the current turn but keep the
	// agent process alive so the conversation can continue.
	if session.transport != nil {
		_ = session.transport.Cancel(ctx, session.ACPSessionID)
	}
	c.persistLocked()

	// Emit cancellation event.
	c.emit(interfaces.Event{
		Type:      interfaces.EventSessionCancelled,
		SessionID: sessionID,
		Timestamp: time.Now().UTC(),
	})

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
		// Cancel every outstanding terminal's run context and kill its
		// subprocess before tearing down the transport, so terminal goroutines
		// and child processes do not outlive the session.
		if session.impl != nil {
			session.impl.releaseAllTerminals()
		}
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
		// Cancel every outstanding terminal's run context and kill its
		// subprocess before tearing down the transport, so terminal goroutines
		// and child processes do not outlive a daemon shutdown.
		if session.impl != nil {
			session.impl.releaseAllTerminals()
		}
		// Best-effort ACP session/delete before killing the process. Errors are
		// ignored: the agent may not advertise session/delete (unstable) or the
		// subprocess may already have exited.
		if session.ACPSessionID != "" {
			_ = session.transport.DeleteSession(ctx, session.ACPSessionID)
		}
		_ = session.transport.Close()
		session.transport = nil
		session.impl = nil
	}

	// Drop cached permission policies for this session so allow_always /
	// allow_session decisions do not leak into future sessions reusing the ID.
	if c.permMgr != nil {
		c.permMgr.ClearSession(session.ID)
	}

	// Mark idle so the UI does not show the session as "running" after restart.
	// LoadConversations also resets status to "idle", but setting it here keeps
	// the on-disk file consistent in case the daemon is inspected post-shutdown.
	session.Status = statusIdle
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
// one starts on the next prompt. The prior conversation is exported as a
// markdown transcript and queued for injection into the new agent's first
// prompt so it can continue the conversation with context. The per-session
// prompt counter is reset so first-prompt middlewares (workspace context and
// the conversation transfer) fire again.
func (c *Client) RebindSession(ctx context.Context, sessionID, agentID, modelID string, maxTransferBytes int) (interfaces.SessionInfo, error) {
	c.mu.Lock()
	defer c.mu.Unlock()

	session, ok := c.sessions[sessionID]
	if !ok {
		return interfaces.SessionInfo{}, fmt.Errorf("session not found: %s", sessionID)
	}

	agent, err := c.validateAgentModelLocked(agentID, modelID)
	if err != nil {
		return interfaces.SessionInfo{}, err
	}

	// Capture the old agent's display name before switching, for the transfer
	// header ("transferred from {agentName}").
	oldAgentName := ""
	if oldAgent, ok := c.agents[session.AgentID]; ok {
		oldAgentName = oldAgent.Name
	}

	// Export the prior conversation history as a markdown transcript before
	// tearing down the transport. The events live in the event store
	// independently of the transport, so the export is valid even after the
	// transport is closed — but doing it first keeps the ordering clear. The
	// byte budget comes from the system-message config so a long history does
	// not blow past the new agent's context window.
	var conversationMarkdown string
	if c.eventStore != nil && c.transfer != nil {
		// Use the caller-provided limit when > 0; otherwise fall back to the
		// config default. 0 or negative means "no limit" (full transcript).
		maxBytes := maxTransferBytes
		if maxBytes == 0 && c.transfer.Messages != nil {
			maxBytes = c.transfer.Messages.MaxContextBytes
		}
		md, exportErr := ExportConversation(ctx, c.eventStore, sessionID, maxBytes)
		if exportErr != nil {
			// Best-effort: log via the event content and continue without a
			// transfer rather than failing the rebind.
			conversationMarkdown = fmt.Sprintf("[conversation export failed: %s]", exportErr)
		} else {
			conversationMarkdown = md
		}
	}

	// Tear down the old transport; it restarts lazily on the next prompt.
	// Release outstanding terminals first so their subprocesses are killed.
	if session.transport != nil {
		if session.impl != nil {
			session.impl.releaseAllTerminals()
		}
		_ = session.transport.Close()
		session.transport = nil
		session.impl = nil
		session.ACPSessionID = ""
	}

	session.AgentID = agentID
	session.ModelID = modelID
	session.Status = statusIdle
	session.UpdatedAt = time.Now().UTC()
	c.persistLocked()

	// Reset the per-session prompt counter so first-prompt middlewares
	// (workspace context and the conversation transfer) fire on the new
	// agent's first prompt.
	if c.pipeline != nil {
		c.pipeline.Reset(sessionID)
	}

	// Queue the exported transcript so the ConversationTransferMiddleware
	// injects it into the new agent's first prompt.
	if c.transfer != nil && strings.TrimSpace(conversationMarkdown) != "" {
		c.transfer.SetTransfer(sessionID, conversationMarkdown, oldAgentName)
	}

	c.emit(interfaces.Event{
		Type:      interfaces.EventConnectionRestarted,
		SessionID: sessionID,
		Timestamp: time.Now().UTC(),
		Content:   fmt.Sprintf("Switched to %s / %s — prior history exported and will be injected as context for the new agent.", agent.Name, modelID),
	})

	return sessionToInfo(session), nil
}

// SwitchModel changes the model on a live session without restarting the agent
// process. Uses ACP's session/set_config_option (category "model") when the
// agent advertises a model config option. Falls back to RebindSession only when
// the agent genuinely doesn't support model config options (older agents).
//
// If the transport is not currently live (e.g. closed on daemon shutdown and
// not yet restarted), it is lazily started first — same as SendPrompt does —
// which runs session/load (or session/new) and re-derives modelConfigID from
// the agent's advertised config options. Only if modelConfigID is still empty
// after the handshake do we fall back to rebind.
//
// Unlike RebindSession, the live-session path preserves the full conversation
// context — the agent keeps its in-memory state and just uses the new model for
// subsequent turns.
func (c *Client) SwitchModel(ctx context.Context, sessionID, modelID string) error {
	c.mu.Lock()
	session, ok := c.sessions[sessionID]
	if !ok {
		c.mu.Unlock()
		return fmt.Errorf("session not found: %s", sessionID)
	}

	// Lazily (re)start the agent process — sessions loaded from disk after a
	// daemon restart start without a live transport. Starting it now calls
	// session/load (or session/new) and re-derives modelConfigID from the
	// agent's advertised config options. Without this, a post-restart model
	// switch would fall back to RebindSession (which exports history and starts
	// a fresh agent session) even though the agent supports
	// session/set_config_option and could resume cleanly.
	if session.transport == nil {
		if err := c.startTransportLocked(ctx, session); err != nil {
			c.mu.Unlock()
			return fmt.Errorf("start transport for model switch: %w", err)
		}
	}

	// After (re)starting the transport, check whether the agent advertises a
	// model config option. If not, fall back to rebind — the agent genuinely
	// doesn't support live model switching.
	if session.modelConfigID == "" {
		agentID := session.AgentID
		c.mu.Unlock()
		slog.Info("agent does not support model config option; falling back to rebind", "session", sessionID, "model", modelID)
		_, err := c.RebindSession(ctx, sessionID, agentID, modelID, 0)
		return err
	}

	configID := session.modelConfigID
	acpSessionID := session.ACPSessionID
	transport := session.transport
	session.ModelID = modelID
	session.UpdatedAt = time.Now().UTC()
	c.mu.Unlock()

	if err := transport.SetSessionConfigOption(ctx, acpSessionID, configID, modelID); err != nil {
		return fmt.Errorf("set model config option: %w", err)
	}

	// Persist the model change to disk so it survives a daemon restart.
	c.mu.Lock()
	c.persistLocked()
	c.mu.Unlock()

	// Emit a lightweight event so the UI knows the model changed. This is
	// NOT ConnectionRestarted — that implies history was reset, which did
	// not happen here.
	c.emit(interfaces.Event{
		Type:      interfaces.EventModelChanged,
		SessionID: sessionID,
		Timestamp: time.Now().UTC(),
		Content:   fmt.Sprintf("Switched model to %s.", modelID),
	})
	return nil
}

// ListProviders returns the agent's configurable LLM providers for the given
// session, with their current routing info. The transport is lazily (re)started
// if nil (same pattern as SwitchModel/SendPrompt). Returns ErrProvidersUnsupported
// when the agent did not advertise the providers capability during Initialize.
func (c *Client) ListProviders(ctx context.Context, sessionID string) ([]interfaces.ProviderInfo, error) {
	c.mu.Lock()
	session, ok := c.sessions[sessionID]
	if !ok {
		c.mu.Unlock()
		return nil, fmt.Errorf("session not found: %s", sessionID)
	}
	if session.transport == nil {
		if err := c.startTransportLocked(ctx, session); err != nil {
			c.mu.Unlock()
			return nil, fmt.Errorf("start transport for providers list: %w", err)
		}
	}
	if !session.transport.SupportsProviders() {
		c.mu.Unlock()
		return nil, ErrProvidersUnsupported
	}
	transport := session.transport
	c.mu.Unlock()

	providers, err := transport.ListProviders(ctx)
	if err != nil {
		return nil, fmt.Errorf("list providers: %w", err)
	}
	return toInterfacesProviders(providers), nil
}

// SetProvider configures a single LLM provider on the agent for the given
// session. id is the provider identifier (e.g. "main", "openai"); apiType is one
// of the ACP LLM protocol strings ("anthropic","openai","azure","vertex",
// "bedrock"); baseURL is the endpoint; headers is an optional map of
// integration-specific headers (e.g. authorization). The transport is lazily
// (re)started if nil. Returns ErrProvidersUnsupported when the agent did not
// advertise the providers capability.
func (c *Client) SetProvider(ctx context.Context, sessionID, id, apiType, baseURL string, headers map[string]string) error {
	c.mu.Lock()
	session, ok := c.sessions[sessionID]
	if !ok {
		c.mu.Unlock()
		return fmt.Errorf("session not found: %s", sessionID)
	}
	if session.transport == nil {
		if err := c.startTransportLocked(ctx, session); err != nil {
			c.mu.Unlock()
			return fmt.Errorf("start transport for set provider: %w", err)
		}
	}
	if !session.transport.SupportsProviders() {
		c.mu.Unlock()
		return ErrProvidersUnsupported
	}
	transport := session.transport
	c.mu.Unlock()

	// Convert map[string]string → map[string]any for the SDK. A nil map is
	// passed through as nil so the field is omitted on the wire.
	var hdrs map[string]any
	if len(headers) > 0 {
		hdrs = make(map[string]any, len(headers))
		for k, v := range headers {
			hdrs[k] = v
		}
	}
	if err := transport.SetProvider(ctx, id, apiType, baseURL, hdrs); err != nil {
		return fmt.Errorf("set provider: %w", err)
	}
	return nil
}

// DisableProvider disables the LLM provider with the given id on the agent for
// the given session. The transport is lazily (re)started if nil. Returns
// ErrProvidersUnsupported when the agent did not advertise the providers
// capability. Callers MUST check the Required flag (via ListProviders) before
// calling this — the spec forbids disabling a required provider, and the agent
// will reject it. The REST handler enforces this guard.
func (c *Client) DisableProvider(ctx context.Context, sessionID, id string) error {
	c.mu.Lock()
	session, ok := c.sessions[sessionID]
	if !ok {
		c.mu.Unlock()
		return fmt.Errorf("session not found: %s", sessionID)
	}
	if session.transport == nil {
		if err := c.startTransportLocked(ctx, session); err != nil {
			c.mu.Unlock()
			return fmt.Errorf("start transport for disable provider: %w", err)
		}
	}
	if !session.transport.SupportsProviders() {
		c.mu.Unlock()
		return ErrProvidersUnsupported
	}
	transport := session.transport
	c.mu.Unlock()

	if err := transport.DisableProvider(ctx, id); err != nil {
		return fmt.Errorf("disable provider: %w", err)
	}
	return nil
}

// toInterfacesProviders converts the SDK's []acp.UnstableProviderInfo into the
// interface-layer []interfaces.ProviderInfo projection. Supported protocols
// (acp.UnstableLlmProtocol) are stringified so the server/UI never depend on
// the SDK type.
func toInterfacesProviders(in []acp.UnstableProviderInfo) []interfaces.ProviderInfo {
	out := make([]interfaces.ProviderInfo, 0, len(in))
	for _, p := range in {
		pi := interfaces.ProviderInfo{
			ID:       p.Id,
			Required: p.Required,
		}
		for _, s := range p.Supported {
			pi.Supported = append(pi.Supported, string(s))
		}
		if p.Current != nil {
			pi.Current = &interfaces.ProviderCurrentConfig{
				APIType: string(p.Current.ApiType),
				BaseURL: p.Current.BaseUrl,
			}
		}
		out = append(out, pi)
	}
	return out
}

// SetTransportForTest injects a mock transport into an existing session so
// tests in other packages (e.g. internal/server) can exercise the
// provider-management Client methods without spawning a real agent process.
// The session must already exist (create it via LoadConversations from a seed
// file). This is intended for testing only.
func (c *Client) SetTransportForTest(sessionID string, t TransportLike) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	session, ok := c.sessions[sessionID]
	if !ok {
		return fmt.Errorf("session not found: %s", sessionID)
	}
	session.transport = t
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

// selectAgentAuthMethod returns the ID of the first agent-handled auth method
// advertised in the Initialize response. Agent methods (browser PKCE, device
// code, etc.) are ones the agent can complete without the client collecting
// secrets. Env-var and terminal methods need interactive credential input that
// is not yet wired into the UI, so they are skipped. Returns "" when no
// suitable method is available (including when the agent requires no auth).
func selectAgentAuthMethod(methods []acp.AuthMethod) string {
	for _, m := range methods {
		if m.Agent != nil && m.Agent.Id != "" {
			return m.Agent.Id
		}
	}
	return ""
}

func generateSessionID() (string, error) {
	b := make([]byte, 8)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return "sess-" + hex.EncodeToString(b), nil
}
