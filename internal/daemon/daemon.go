// Package daemon manages the lifecycle of the Local Agent Interface daemon.
// Blueprint references: Sec 4 (Host Daemon), Sec 20 (Configuration).
package daemon

import (
	"context"
	"fmt"
	"log"
	"os"
	"os/signal"
	"path/filepath"
	"strconv"
	"syscall"

	"github.com/adama/local-agent/internal/acp"
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
	Port    int    `json:"port"`
	Host    string `json:"host"`
	DataDir string `json:"dataDir"`
	DBPath  string `json:"dbPath"`
}

// DefaultConfig returns the default daemon configuration.
func DefaultConfig() *Config {
	homeDir, err := os.UserHomeDir()
	if err != nil {
		homeDir = "."
	}
	dataDir := filepath.Join(homeDir, ".local-agent")

	return &Config{
		Port:    7337,
		Host:    "0.0.0.0",
		DataDir: dataDir,
		DBPath:  filepath.Join(dataDir, "local-agent.db"),
	}
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
	workspaceMgr := workspace.NewManager()
	acpClient := acp.NewClient()
	permissionMgr := permissions.NewManager()
	syncHub := sync.NewHub()

	// Register a default agent so the UI has something to show.
	// In production, agents are discovered via ACP capability negotiation.
	acpClient.RegisterAgent(acp.AgentInfo{
		ID:      "claude-code",
		Name:    "Claude Code",
		Command: "claude",
		Models: []acp.AgentModel{
			{ID: "claude-sonnet-4", Name: "Claude Sonnet 4"},
			{ID: "claude-opus-4", Name: "Claude Opus 4"},
		},
	})

	// Create the server with all dependencies wired in.
	srv := server.New(&server.Deps{
		EventStore:    eventStore,
		PairingMgr:    pairingMgr,
		WorkspaceMgr:  workspaceMgr,
		ACPClient:     acpClient,
		PermissionMgr: permissionMgr,
		SyncHub:       syncHub,
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

	// Start HTTP server in a goroutine.
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

// cleanup closes resources during shutdown.
func (d *Daemon) cleanup() {
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
