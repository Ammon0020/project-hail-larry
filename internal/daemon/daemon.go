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
	"syscall"

	"github.com/adama/local-agent/internal/server"
)

// Config holds daemon configuration loaded from ~/.local-agent/.
type Config struct {
	Port       int    `json:"port"`
	Host       string `json:"host"`
	DataDir    string `json:"dataDir"`
	DBPath     string `json:"dbPath"`
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
}

// New creates a new Daemon with the given configuration.
func New(cfg *Config) *Daemon {
	return &Daemon{
		config: cfg,
		server: server.New(),
	}
}

// Start runs the daemon until the context is cancelled or a signal is received.
func (d *Daemon) Start(ctx context.Context) error {
	// Ensure data directory exists.
	if err := os.MkdirAll(d.config.DataDir, 0755); err != nil {
		return fmt.Errorf("create data dir: %w", err)
	}

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
		return err
	case <-ctx.Done():
		log.Println("Shutting down daemon...")
		return nil
	}
}
