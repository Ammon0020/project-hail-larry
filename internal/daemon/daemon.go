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
	"github.com/adama/local-agent/internal/pairing"
	"github.com/adama/local-agent/internal/permissions"
	"github.com/adama/local-agent/internal/server"
	"github.com/adama/local-agent/internal/sync"
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
}

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
	// Inject workspace context (file tree, git status, AGENTS.md) into the
	// first prompt of each session so agents don't shell out to discover files.
	acpClient.SetPipeline(acp.NewPromptPipeline(acp.NewFirstPromptContextMiddleware(workspaceMgr)))
	// Persist conversation metadata so chats are remembered across restarts.
	acpClient.SetStorePath(filepath.Join(cfg.DataDir, "conversations.json"))
	if err := acpClient.LoadConversations(); err != nil {
		log.Printf("WARNING: failed to load conversations: %v", err)
	}
	syncHub := sync.NewHub()

	// Load persisted agents from config and run autodetection
	activeAgents, changed := mergeAutodetectedAgents(appCfg.Agents, acp.Autodetect())

	// Verify executables and register
	for i := range activeAgents {
		// Use acp.AgentInfo struct defined in acp.go
		if _, err := os.Stat(activeAgents[i].Command); err != nil {
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

	// Create the server with all dependencies wired in.
	srv := server.New(&server.Deps{
		EventStore:    eventStore,
		PairingMgr:    pairingMgr,
		WorkspaceMgr:  workspaceMgr,
		ACPClient:     acpClient,
		PermissionMgr: permissionMgr,
		SyncHub:       syncHub,
		Config:        appCfg,
	})

	return &Daemon{
		config:        cfg,
		server:        srv,
		eventStore:    eventStore,
		pairingMgr:    pairingMgr,
		workspaceMgr:  workspaceMgr,
		acpClient:     acpClient,
		permissionMgr: permissionMgr,
		syncHub:       syncHub,
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
