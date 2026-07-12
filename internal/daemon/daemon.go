// Package daemon manages the lifecycle of the Local Agent Interface daemon.
// Blueprint references: Sec 4 (Host Daemon), Sec 20 (Configuration).
package daemon

import (
	"context"
	"fmt"
	"log"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/adama/local-agent/internal/acp"
	"github.com/adama/local-agent/internal/config"
	"github.com/adama/local-agent/internal/events"
	"github.com/adama/local-agent/internal/fswatch"
	"github.com/adama/local-agent/internal/pairing"
	"github.com/adama/local-agent/internal/permissions"
	"github.com/adama/local-agent/internal/server"
	"github.com/adama/local-agent/internal/sync"
	"github.com/adama/local-agent/internal/uploads"
	"github.com/adama/local-agent/internal/workspace"
)

const (
	appDataDirPerm = 0700
	pidFilePerm    = 0600

	// defaultBindHost is the all-interfaces bind address used when the user
	// doesn't override Host in config. An empty Host is treated equivalently.
	defaultBindHost = "0.0.0.0"

	// warningExecutableNotFound is set on an agent entry when its launch
	// command is neither a valid file nor on PATH. Cleared once it resolves.
	warningExecutableNotFound = "Executable not found in PATH"
)

// Config holds daemon configuration loaded from ~/.local-agent/.
type Config struct {
	Port       int    `json:"port"`
	Host       string `json:"host"`
	DataDir    string `json:"dataDir"`
	DBPath     string `json:"dbPath"`
	TLSEnabled bool   `json:"tlsEnabled"`
	TLSCertDir string `json:"tlsCertDir"`
	// HTTPSPort is the TCP port for the HTTPS listener in dual HTTP+HTTPS
	// mode (used when TLSEnabled is true). 0 means "Port + 1" at runtime.
	HTTPSPort         int `json:"httpsPort,omitempty"`
	PairingTTLSeconds int `json:"pairingTtlSeconds"`
	// CredentialInactivityTTLSeconds is the sliding-window inactivity expiry for
	// paired device credentials, in seconds. > 0 enables sliding expiry (a device
	// idle this long must re-pair); 0 disables it (credentials never expire). It
	// defaults to 30 days (see DefaultConfigOrError).
	CredentialInactivityTTLSeconds int `json:"credentialInactivityTtlSeconds"`
	// AllowRemoteWorkspaceRegistration controls whether paired devices may
	// register new workspace directories from the web UI / remote API. It
	// defaults to false: workspaces are registered from the host via the
	// `app add-folder <path>` CLI command. Set to true to allow remote
	// registration (still gated by the grace-period pending action flow).
	AllowRemoteWorkspaceRegistration bool `json:"allowRemoteWorkspaceRegistration"`
	// RevocationGracePeriodSeconds is the grace period (in seconds) that a
	// device revocation or remote workspace registration spends in a pending
	// state before being executed. Defaults to 300 (5 minutes); 0 means
	// instant execution (no grace period).
	RevocationGracePeriodSeconds int `json:"revocationGracePeriodSeconds"`
}

// defaultCredentialInactivityTTLSeconds is the default sliding-window credential
// inactivity expiry (30 days) applied to a default daemon config.
const defaultCredentialInactivityTTLSeconds = 2592000

// DefaultConfig returns the default daemon configuration. It panics if the
// user's home directory cannot be determined, since silently falling back to
// the current working directory would write the SQLite database and TLS keys
// to an unpredictable location. Callers that need to handle this case should
// use DefaultConfigOrError.
func DefaultConfig() *Config {
	cfg, err := DefaultConfigOrError()
	if err != nil {
		panic(fmt.Sprintf("determine home directory for default config: %v", err))
	}
	return cfg
}

// DefaultConfigOrError returns the default daemon configuration or an error if
// the user's home directory cannot be determined. This is the error-returning
// variant of DefaultConfig for callers that want to handle the failure
// gracefully instead of panicking.
func DefaultConfigOrError() (*Config, error) {
	homeDir, err := os.UserHomeDir()
	if err != nil {
		return nil, fmt.Errorf("determine home directory: %w", err)
	}
	dataDir := filepath.Join(homeDir, ".local-agent")

	return &Config{
		Port:    7337,
		Host:    defaultBindHost,
		DataDir: dataDir,
		DBPath:  filepath.Join(dataDir, "local-agent.db"),
		// TLSEnabled defaults to true so the daemon runs in dual HTTP+HTTPS
		// mode: HTTP on Port (cleartext for LAN home use) and HTTPS on
		// HTTPSPort (Port+1 by default) for coffee-shop TLS. The self-signed
		// cert generation (internal/server/tls.go EnsureSelfSignedCert)
		// covers localhost, 127.0.0.1, and all LAN IPs. Users who want plain
		// HTTP only can set "tlsEnabled": false in ~/.local-agent/config.json.
		TLSEnabled:        true,
		TLSCertDir:        filepath.Join(dataDir, "tls"),
		PairingTTLSeconds: 300,

		CredentialInactivityTTLSeconds: defaultCredentialInactivityTTLSeconds,

		// Workspaces are registered from the host CLI by default; remote
		// registration is gated behind a grace-period pending action and is
		// off unless the user explicitly enables it.
		AllowRemoteWorkspaceRegistration: false,
		// Default 5-minute grace period for destructive actions (device
		// revocation, remote workspace registration) so a stolen device
		// cannot lock the user out or register sensitive directories before
		// the user's other devices can cancel.
		RevocationGracePeriodSeconds: 300,
	}, nil
}

func mergeAutodetectedAgents(configured, detected []acp.AgentInfo) ([]acp.AgentInfo, bool) {
	merged := append([]acp.AgentInfo(nil), configured...)
	changed := false

	for _, detectedAgent := range detected {
		found := false
		for i := range merged {
			if merged[i].ID != detectedAgent.ID {
				continue
			}
			found = true
			if merged[i].Name == "" {
				merged[i].Name = detectedAgent.Name
				changed = true
			}
			if merged[i].Command == "" {
				merged[i].Command = detectedAgent.Command
				changed = true
			}
			if len(merged[i].Models) == 0 {
				merged[i].Models = detectedAgent.Models
				changed = true
			}
			if merged[i].Warning == "" {
				merged[i].Warning = detectedAgent.Warning
			}
			break
		}
		if !found {
			merged = append(merged, detectedAgent)
			changed = true
		}
	}

	return merged, changed
}

// pruneStaleKnownAgents removes persisted agent entries whose ID matches a
// known agent spec but whose Command is no longer a valid launch command for
// that spec. This is a one-time cleanup migration that runs at daemon startup
// to repair configs left behind by spec changes — e.g. the "codex" spec used
// to accept the bare "codex" TUI binary, which cannot speak ACP and now that
// the spec only lists "codex-acp", any persisted entry pointing at the TUI is
// stale and would otherwise be re-merged on every launch.
//
// Entries whose ID does not match a known spec (user-defined custom agents)
// are always preserved. Returns the filtered slice and whether anything was
// removed.
func pruneStaleKnownAgents(agents []acp.AgentInfo) ([]acp.AgentInfo, bool) {
	pruned := make([]acp.AgentInfo, 0, len(agents))
	removed := false
	for _, a := range agents {
		validCommands := acp.ValidCommandsForAgent(a.ID)
		if validCommands == nil {
			// Unknown / user-defined agent — keep it.
			pruned = append(pruned, a)
			continue
		}
		if commandMatchesSpec(a.Command, validCommands) {
			pruned = append(pruned, a)
			continue
		}
		log.Printf("WARNING: removing stale agent entry %q: command %q is not a valid launch command for this agent (expected one of %v)",
			a.ID, a.Command, validCommands)
		removed = true
	}
	return pruned, removed
}

// commandMatchesSpec reports whether cmd is one of the spec's valid command
// names, either as a bare name or as a filesystem path whose base name equals
// one of them (e.g. "/home/user/.nvm/.../bin/codex-acp" matches "codex-acp").
// On Windows the base name comparison is case-insensitive and ignores the
// .exe/.cmd extension so a persisted "C:\\path\\codex-acp.exe" still matches.
func commandMatchesSpec(cmd string, validCommands []string) bool {
	if cmd == "" {
		return false
	}
	base := filepath.Base(cmd)
	for _, valid := range validCommands {
		if cmd == valid || base == valid {
			return true
		}
		if runtime.GOOS == "windows" {
			if strings.EqualFold(base, valid) ||
				strings.EqualFold(strings.TrimSuffix(base, ".exe"), valid) ||
				strings.EqualFold(strings.TrimSuffix(base, ".cmd"), valid) {
				return true
			}
		}
	}
	return false
}

// Daemon is the background process that serves the web UI and API.
type Daemon struct {
	config *Config
	server *server.Server

	// Managers for cleanup on shutdown.
	eventStore    *events.Store
	pairingMgr    *pairing.Manager
	workspaceMgr  *workspace.Manager
	acpClient     *acp.Client
	permissionMgr *permissions.Manager
	syncHub       *sync.Hub
	fsWatcher     *fswatch.Watcher
	uploadsMgr    *uploads.Manager
}

// New creates a new Daemon with the given configuration.
// It initializes all managers and wires them into the server.
func New(cfg *Config) (*Daemon, error) {
	// Ensure data directory exists before opening the database.
	if err := os.MkdirAll(cfg.DataDir, appDataDirPerm); err != nil {
		return nil, fmt.Errorf("create data dir: %w", err)
	}

	// Initialize the event store (SQLite).
	eventStore, err := events.New(cfg.DBPath)
	if err != nil {
		return nil, fmt.Errorf("init event store: %w", err)
	}

	// Initialize all managers.
	pairingMgr := pairing.NewManager(cfg.DataDir)
	if cfg.PairingTTLSeconds > 0 {
		pairingMgr.SetTTL(time.Duration(cfg.PairingTTLSeconds) * time.Second)
	}
	// Wire the sliding-window credential inactivity expiry. A value > 0 enables
	// it; 0 (or unset) leaves the manager's default of "never expire" in place.
	if cfg.CredentialInactivityTTLSeconds > 0 {
		pairingMgr.SetInactivityTTL(time.Duration(cfg.CredentialInactivityTTLSeconds) * time.Second)
	}
	workspaceMgr := workspace.NewManager()

	// Wire the workspace registration callback into the pairing manager so
	// grace-period pending workspace registrations can execute against the
	// workspace manager once their timer fires. The pairing package cannot
	// import workspace (it would create an import cycle), so the daemon sets
	// this callback after both managers are created.
	pairingMgr.SetWorkspaceRegisterFn(func(path string) error {
		_, regErr := workspaceMgr.Register(context.Background(), path)
		return regErr
	})

	// Load persisted workspaces from config.
	appCfg, err := config.Load()
	if err != nil {
		log.Printf("WARNING: failed to load config: %v", err)
		appCfg = config.Default()
	}
	for _, wsPath := range appCfg.Workspaces {
		if _, regErr := workspaceMgr.Register(context.Background(), wsPath); regErr != nil {
			log.Printf("WARNING: failed to load workspace %s: %v", wsPath, regErr)
		}
	}

	permissionMgr := permissions.NewManager()
	acpClient := acp.NewClient(workspaceMgr, permissionMgr)
	// Load externalized system-message templates (header strings + numeric
	// limits) for the prompt middleware pipeline. Falls back to built-in
	// defaults when the config file is missing or unreadable.
	systemMessages, _ := acp.LoadSystemMessages("configs/system-messages.json")
	// OpenFilesTracker holds the open/recently-edited file paths reported by
	// the frontend via POST /api/sessions/{id}/context. It starts empty; the
	// middlewares skip injection until the frontend reports state.
	openFilesTracker := acp.NewOpenFilesTracker()
	// ConversationTransferMiddleware queues an exported conversation transcript
	// for injection into the first prompt of a session rebound to a new agent.
	// RebindSession calls SetTransfer on it after exporting the prior history.
	conversationTransfer := acp.NewConversationTransferMiddleware(systemMessages)
	// ProfileMiddleware injects Code/Ask/Plan system instructions before each
	// prompt. It is registered in the pipeline and also wired into the ACP
	// client so the REST handler can set per-session profiles.
	profileMiddleware := acp.NewProfileMiddleware(systemMessages)
	// Inject workspace context (file tree, git status, AGENTS.md) into the
	// first prompt of each session so agents don't shell out to discover files,
	// plus per-prompt time/open-files/recent-edits context. The conversation
	// transfer middleware runs after the first-prompt context so the workspace
	// bundle comes first, then the transferred conversation, then the profile.
	acpClient.SetPipeline(acp.NewPromptPipeline(
		acp.NewFirstPromptContextMiddleware(workspaceMgr, systemMessages),
		acp.NewTimeMiddleware(systemMessages),
		acp.NewOpenFilesMiddleware(openFilesTracker, systemMessages),
		acp.NewOpenFilesResourceMiddleware(openFilesTracker, workspaceMgr, systemMessages),
		acp.NewRecentEditsMiddleware(openFilesTracker, systemMessages),
		conversationTransfer,
		profileMiddleware,
	))
	acpClient.SetProfileMiddleware(profileMiddleware)
	// Give the client access to the event store (for conversation export on
	// rebind) and the transfer middleware (so RebindSession can queue the
	// exported transcript for the new agent's first prompt).
	acpClient.SetEventStore(eventStore)
	acpClient.SetConversationTransfer(conversationTransfer)
	// MCP server config: load ~/.local-agent/mcp.json at session start and
	// pass the enabled, capability-filtered server list to the agent on
	// session/new and session/load. The same path is exposed to the server for
	// the /api/mcp REST endpoints.
	mcpConfigPath := filepath.Join(cfg.DataDir, "mcp.json")
	acpClient.SetMcpConfigPath(mcpConfigPath)
	// Persist conversation metadata so chats are remembered across restarts.
	acpClient.SetStorePath(filepath.Join(cfg.DataDir, "conversations.json"))
	if loadErr := acpClient.LoadConversations(); loadErr != nil {
		log.Printf("WARNING: failed to load conversations: %v", loadErr)
	}
	syncHub := sync.NewHub()

	// One-time cleanup migration: prune persisted agent entries whose ID
	// matches a known agent spec but whose Command is no longer valid for
	// that spec (e.g. a stale "codex" entry pointing at the bare codex TUI
	// binary instead of the codex-acp adapter). This runs before autodetect
	// so the merge below doesn't re-add the stale command from the persisted
	// copy. Unknown / user-defined agents are always preserved.
	if pruned, removed := pruneStaleKnownAgents(appCfg.Agents); removed {
		appCfg.Agents = pruned
		if saveErr := appCfg.Save(); saveErr != nil {
			log.Printf("WARNING: failed to persist pruned agent config: %v", saveErr)
		}
	}

	// Load persisted agents from config and run autodetection
	activeAgents, changed := mergeAutodetectedAgents(appCfg.Agents, acp.Autodetect())

	// Verify executables and register
	for i := range activeAgents {
		// Use acp.AgentInfo struct defined in acp.go
		if _, statErr := os.Stat(activeAgents[i].Command); statErr != nil {
			// fallback to LookPath
			if _, lpErr := exec.LookPath(activeAgents[i].Command); lpErr != nil {
				activeAgents[i].Warning = warningExecutableNotFound
			} else if activeAgents[i].Warning == warningExecutableNotFound {
				activeAgents[i].Warning = ""
			}
		} else if activeAgents[i].Warning == warningExecutableNotFound {
			activeAgents[i].Warning = ""
		}
		acpClient.RegisterAgent(activeAgents[i])
	}

	if changed && appCfg != nil {
		appCfg.Agents = activeAgents
		_ = appCfg.Save()
	}

	// Per-session uploads store for artifacts attached to prompts (e.g.
	// images). A failure to initialize is non-fatal — the daemon runs without
	// upload support, and the server handlers return a 400 (send prompt) or
	// 503 (upload endpoints) when Uploads is nil.
	uploadsMgr, err := uploads.New(filepath.Join(cfg.DataDir, "uploads"))
	if err != nil {
		log.Printf("WARNING: uploads manager unavailable: %v", err)
	}

	// Create the server with all dependencies wired in.
	srv := server.New(&server.Deps{
		EventStore:       eventStore,
		PairingMgr:       pairingMgr,
		WorkspaceMgr:     workspaceMgr,
		ACPClient:        acpClient,
		PermissionMgr:    permissionMgr,
		SyncHub:          syncHub,
		Config:           appCfg,
		OpenFilesTracker: openFilesTracker,
		Uploads:          uploadsMgr,
		McpConfigPath:    mcpConfigPath,
	})

	// Register the grace-period pending-action routes (cancel-revocation,
	// pending-actions list, cancel-workspace-registration). These live in
	// api.go but are wired here so server.go's apiRoutes() does not need to
	// be modified. server.New already registered the core routes; this adds
	// the new ones on the same mux.
	srv.RegisterPendingActionRoutes()

	// Filesystem watcher: detect external file changes (edits made outside the
	// app) and broadcast EventFileChangedOnDisk through the server's event sink
	// (the same path as agent writes). The workspace manager notifies it of the
	// app's own writes (to suppress them) and of workspace add/remove. A watcher
	// init failure is non-fatal — the daemon still runs, just without external
	// change detection.
	fsWatcher, werr := fswatch.New(srv.OnEvent)
	if werr != nil {
		log.Printf("WARNING: filesystem watcher unavailable: %v", werr)
	} else {
		workspaceMgr.SetOnWrite(fsWatcher.NoteAppWrite)
		workspaceMgr.SetOnRegister(fsWatcher.AddWorkspace)
		workspaceMgr.SetOnRemove(fsWatcher.RemoveWorkspace)
		// Workspaces registered at startup (above) predate the watcher, so add
		// them now; runtime registrations go through the SetOnRegister hook.
		if wss, lerr := workspaceMgr.List(context.Background()); lerr == nil {
			for _, ws := range wss {
				fsWatcher.AddWorkspace(ws.ID, ws.Path)
			}
		}
	}

	return &Daemon{
		config:        cfg,
		server:        srv,
		eventStore:    eventStore,
		pairingMgr:    pairingMgr,
		workspaceMgr:  workspaceMgr,
		acpClient:     acpClient,
		permissionMgr: permissionMgr,
		syncHub:       syncHub,
		fsWatcher:     fsWatcher,
		uploadsMgr:    uploadsMgr,
	}, nil
}

// Start runs the daemon until the context is cancelled or a signal is received.
// It writes a PID file to the data directory for stop/status commands.
func (d *Daemon) Start(ctx context.Context) error {
	// Ensure data directory exists.
	if err := os.MkdirAll(d.config.DataDir, appDataDirPerm); err != nil {
		return fmt.Errorf("create data dir: %w", err)
	}

	// Write PID file for stop/status commands.
	pidFile := filepath.Join(d.config.DataDir, "daemon.pid")
	if err := os.WriteFile(pidFile, []byte(strconv.Itoa(os.Getpid())), pidFilePerm); err != nil {
		return fmt.Errorf("write pid file: %w", err)
	}
	defer func() {
		_ = os.Remove(pidFile)
	}()

	addr := fmt.Sprintf("%s:%d", d.config.Host, d.config.Port)

	// Handle graceful shutdown on SIGINT/SIGTERM.
	ctx, cancel := signal.NotifyContext(ctx, syscall.SIGINT, syscall.SIGTERM)
	defer cancel()

	// Resolve the HTTPS port. A configured HTTPSPort wins; 0 means "Port + 1"
	// so the default dual-stack is 7337 (HTTP) + 7338 (HTTPS) with no extra
	// config. This is computed even when TLS is disabled so the value is
	// deterministic for status/logging if the user inspects it later.
	httpsPort := d.config.HTTPSPort
	if httpsPort == 0 {
		httpsPort = d.config.Port + 1
	}
	httpsAddr := fmt.Sprintf("%s:%d", d.config.Host, httpsPort)

	// Dual HTTP+HTTPS mode: TLS enabled means the daemon listens on BOTH the
	// cleartext HTTP port (for LAN home use) and the TLS HTTPS port (for
	// coffee-shop / untrusted networks). The user picks a scheme by typing
	// http://IP:port or https://IP:httpsPort — no restart, no config flip.
	// HTTP-only mode (TLSEnabled false) keeps the legacy single-listener path.
	if d.config.TLSEnabled {
		certDir := d.config.TLSCertDir
		if certDir == "" {
			certDir = filepath.Join(d.config.DataDir, "tls")
		}
		certPath, keyPath, err := server.EnsureSelfSignedCert(certDir, d.config.Host)
		if err != nil {
			return fmt.Errorf("ensure tls cert: %w", err)
		}
		// SetTLS stores the cert/key paths so the single-server ListenAndServe
		// path (used by tests) still works; the daemon uses ListenDual below.
		d.server.SetTLS(certPath, keyPath)

		log.Printf("Local Agent Interface daemon started")
		log.Printf("  HTTP:  http://%s", addr)
		log.Printf("  HTTPS: https://%s  (self-signed)", httpsAddr)
		log.Printf("Data directory: %s", d.config.DataDir)

		// Start both listeners. ListenDual returns a channel that receives
		// the first non-ErrServerClosed error from either listener; a bind
		// failure on HTTPS tears down HTTP too (fail-fast) so the user is
		// not silently left with only cleartext when they requested dual.
		errCh := d.server.ListenDual(addr, httpsAddr, certPath, keyPath)
		select {
		case err := <-errCh:
			d.cleanup()
			return err
		case <-ctx.Done():
			log.Println("Shutting down daemon...")
			d.cleanup()
			return nil
		}
	}

	// HTTP-only path (TLSEnabled false). Warn loudly when bound to all
	// interfaces: every request — including Bearer tokens — travels in
	// cleartext on the LAN, and there is no HTTPS listener to fall back to.
	if d.config.Host == "" || d.config.Host == defaultBindHost {
		log.Printf("WARNING: TLS is disabled and the HTTP server is bound to 0.0.0.0 — all traffic including credentials will be sent in cleartext on the network. Set tlsEnabled: true in config or bind to 127.0.0.1.")
	}

	errCh := make(chan error, 1)
	go func() {
		errCh <- d.server.ListenAndServe(addr)
	}()

	log.Printf("Local Agent Interface daemon started on http://%s", addr)
	log.Printf("Data directory: %s", d.config.DataDir)

	select {
	case err := <-errCh:
		d.cleanup()
		return err
	case <-ctx.Done():
		log.Println("Shutting down daemon...")
		d.cleanup()
		return nil
	}
}

// cleanup closes resources during shutdown. It gracefully shuts down the HTTP
// server first (so no new/in-flight handlers access the EventStore after it is
// closed), then closes all ACP sessions (best-effort session/delete + process
// termination), and finally tears down the event store. The original shutdown
// context may already be cancelled (SIGINT/SIGTERM), so a fresh background
// context with a short timeout is used.
func (d *Daemon) cleanup() {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	// Shut down the HTTP server before closing the EventStore so in-flight
	// handlers finish (or time out) before the store is closed. Without this,
	// a handler could Append to a closed store (use-after-close).
	// Stop the filesystem watcher first so it can't emit an event (→ EventStore
	// Append) after the store is closed below.
	if d.fsWatcher != nil {
		_ = d.fsWatcher.Close()
	}
	// Stop pending action timers before shutting down the HTTP server so
	// timers don't fire and attempt to broadcast events or write to disk
	// during shutdown.
	if d.pairingMgr != nil {
		d.pairingMgr.Close()
	}
	if d.server != nil {
		_ = d.server.Shutdown(ctx)
	}
	// Shut down the sync hub so all WebSocket pump goroutines exit before
	// the event store closes (otherwise pumps could broadcast on a closed store).
	if d.syncHub != nil {
		d.syncHub.Shutdown()
	}
	if d.acpClient != nil {
		_ = d.acpClient.CloseAllSessions(ctx)
	}
	if d.eventStore != nil {
		_ = d.eventStore.Close()
	}
	// Clean up all per-session upload directories on shutdown. The HTTP server
	// and ACP sessions are already torn down above, so no in-flight handler or
	// agent can still be reading an upload path.
	if d.uploadsMgr != nil {
		if err := d.uploadsMgr.RemoveAll(); err != nil {
			log.Printf("WARNING: failed to clean uploads on shutdown: %v", err)
		}
	}
}

// IsRunning checks whether a daemon is currently running by reading the PID file.
// Returns the PID if running, 0 otherwise.
func IsRunning(dataDir string) (int, error) {
	pidFile := filepath.Join(dataDir, "daemon.pid")
	data, err := os.ReadFile(pidFile) //nolint:gosec // pidFile is constructed from the configured app data directory.
	if err != nil {
		if os.IsNotExist(err) {
			return 0, nil
		}
		return 0, err
	}

	pid, err := strconv.Atoi(string(data))
	if err != nil {
		return 0, fmt.Errorf("parse pid: %w", err)
	}

	// Check if the process is actually running.
	if !processExists(pid) {
		// Stale PID file — clean it up.
		_ = os.Remove(pidFile)
		return 0, nil
	}

	return pid, nil
}

// Stop sends SIGTERM to the running daemon process.
func Stop(dataDir string) error {
	pid, err := IsRunning(dataDir)
	if err != nil {
		return err
	}
	if pid == 0 {
		return fmt.Errorf("daemon is not running")
	}

	proc, err := os.FindProcess(pid)
	if err != nil {
		return fmt.Errorf("find process: %w", err)
	}

	if err := stopProcess(proc); err != nil {
		return fmt.Errorf("stop process: %w", err)
	}

	// Clean up PID file.
	pidFile := filepath.Join(dataDir, "daemon.pid")
	_ = os.Remove(pidFile)

	log.Println("Daemon stopped.")
	return nil
}
