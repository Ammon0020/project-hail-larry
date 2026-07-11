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
	"strconv"
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
)

// Config holds daemon configuration loaded from ~/.local-agent/.
type Config struct {
	Port              int    `json:"port"`
	Host              string `json:"host"`
	DataDir           string `json:"dataDir"`
	DBPath            string `json:"dbPath"`
	TLSEnabled        bool   `json:"tlsEnabled"`
	TLSCertDir        string `json:"tlsCertDir"`
	PairingTTLSeconds int    `json:"pairingTtlSeconds"`
	// CredentialInactivityTTLSeconds is the sliding-window inactivity expiry for
	// paired device credentials, in seconds. > 0 enables sliding expiry (a device
	// idle this long must re-pair); 0 disables it (credentials never expire). It
	// defaults to 30 days (see DefaultConfigOrError).
	CredentialInactivityTTLSeconds int `json:"credentialInactivityTtlSeconds"`
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
		Port:              7337,
		Host:              "0.0.0.0",
		DataDir:           dataDir,
		DBPath:            filepath.Join(dataDir, "local-agent.db"),
		TLSCertDir:        filepath.Join(dataDir, "tls"),
		PairingTTLSeconds: 300,

		CredentialInactivityTTLSeconds: defaultCredentialInactivityTTLSeconds,
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
	// Inject workspace context (file tree, git status, AGENTS.md) into the
	// first prompt of each session so agents don't shell out to discover files,
	// plus per-prompt time/open-files/recent-edits context. The conversation
	// transfer middleware runs after the first-prompt context so the workspace
	// bundle comes first, then the transferred conversation.
	acpClient.SetPipeline(acp.NewPromptPipeline(
		acp.NewFirstPromptContextMiddleware(workspaceMgr, systemMessages),
		acp.NewTimeMiddleware(systemMessages),
		acp.NewOpenFilesMiddleware(openFilesTracker, systemMessages),
		acp.NewOpenFilesResourceMiddleware(openFilesTracker, workspaceMgr, systemMessages),
		acp.NewRecentEditsMiddleware(openFilesTracker, systemMessages),
		conversationTransfer,
	))
	// Give the client access to the event store (for conversation export on
	// rebind) and the transfer middleware (so RebindSession can queue the
	// exported transcript for the new agent's first prompt).
	acpClient.SetEventStore(eventStore)
	acpClient.SetConversationTransfer(conversationTransfer)
	// Persist conversation metadata so chats are remembered across restarts.
	acpClient.SetStorePath(filepath.Join(cfg.DataDir, "conversations.json"))
	if loadErr := acpClient.LoadConversations(); loadErr != nil {
		log.Printf("WARNING: failed to load conversations: %v", loadErr)
	}
	syncHub := sync.NewHub()

	// Load persisted agents from config and run autodetection
	activeAgents, changed := mergeAutodetectedAgents(appCfg.Agents, acp.Autodetect())

	// Verify executables and register
	for i := range activeAgents {
		// Use acp.AgentInfo struct defined in acp.go
		if _, statErr := os.Stat(activeAgents[i].Command); statErr != nil {
			// fallback to LookPath
			if _, lpErr := exec.LookPath(activeAgents[i].Command); lpErr != nil {
				activeAgents[i].Warning = "Executable not found in PATH"
			} else if activeAgents[i].Warning == "Executable not found in PATH" {
				activeAgents[i].Warning = ""
			}
		} else if activeAgents[i].Warning == "Executable not found in PATH" {
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
	})

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

	// If TLS is enabled, generate (or reuse) a self-signed certificate and
	// tell the server to serve over HTTPS.
	scheme := "http"
	if d.config.TLSEnabled {
		certDir := d.config.TLSCertDir
		if certDir == "" {
			certDir = filepath.Join(d.config.DataDir, "tls")
		}
		certPath, keyPath, err := server.EnsureSelfSignedCert(certDir, d.config.Host)
		if err != nil {
			return fmt.Errorf("ensure tls cert: %w", err)
		}
		d.server.SetTLS(certPath, keyPath)
		scheme = "https"
	}

	// Start HTTP server in a goroutine.
	errCh := make(chan error, 1)
	go func() {
		errCh <- d.server.ListenAndServe(addr)
	}()

	log.Printf("Local Agent Interface daemon started on %s://%s", scheme, addr)
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
