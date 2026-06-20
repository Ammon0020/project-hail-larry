// Package main is the CLI entry point for the Local Agent Interface.
// Uses cobra for command structure. Blueprint references: Sec 4 (Host Daemon).
package main

import (
	"context"
	"fmt"
	"os"
	"path/filepath"

	"github.com/adama/local-agent/internal/config"
	"github.com/adama/local-agent/internal/daemon"
	"github.com/spf13/cobra"
)

func main() {
	rootCmd := &cobra.Command{
		Use:   "app",
		Short: "Local Agent Interface — self-hosted AI code editor",
		Long: "A Go daemon that serves a browser-based IDE to devices on your local network. " +
			"Pair devices, orchestrate AI agents via ACP, and edit code from anywhere on your LAN.",
	}

	// app start — launch the daemon
	startCmd := &cobra.Command{
		Use:   "start",
		Short: "Start the Local Agent Interface daemon",
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := config.Load()
			if err != nil {
				return fmt.Errorf("load config: %w", err)
			}

			d, err := daemon.New(&daemon.Config{
				Port:    cfg.Port,
				Host:    cfg.Host,
				DataDir: cfg.DataDir,
				DBPath:  cfg.DBPath,
			})
			if err != nil {
				return fmt.Errorf("init daemon: %w", err)
			}

			return d.Start(context.Background())
		},
	}

	// app status — show daemon info
	statusCmd := &cobra.Command{
		Use:   "status",
		Short: "Show daemon status",
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := config.Load()
			if err != nil {
				return fmt.Errorf("load config: %w", err)
			}

			// Check if daemon is running.
			pid, err := daemon.IsRunning(cfg.DataDir)
			if err != nil {
				return fmt.Errorf("check daemon: %w", err)
			}

			if pid > 0 {
				fmt.Printf("Status:   Running (PID %d)\n", pid)
			} else {
				fmt.Println("Status:   Stopped")
			}
			fmt.Printf("Host:     %s\n", cfg.Host)
			fmt.Printf("Port:     %d\n", cfg.Port)
			fmt.Printf("Data:     %s\n", cfg.DataDir)
			fmt.Printf("Workspaces: %d\n", len(cfg.Workspaces))
			for _, ws := range cfg.Workspaces {
				fmt.Printf("  - %s\n", ws)
			}
			return nil
		},
	}

	// app add-folder — register a workspace (stub)
	addFolderCmd := &cobra.Command{
		Use:   "add-folder [path]",
		Short: "Register a workspace directory",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := config.Load()
			if err != nil {
				return fmt.Errorf("load config: %w", err)
			}

			absPath, err := filepath.Abs(args[0])
			if err != nil {
				return err
			}

			cfg.Workspaces = append(cfg.Workspaces, absPath)
			if err := cfg.Save(); err != nil {
				return fmt.Errorf("save config: %w", err)
			}

			fmt.Printf("Workspace registered: %s\n", absPath)
			return nil
		},
	}

	// app pair — generate QR code and mnemonic (stub)
	pairCmd := &cobra.Command{
		Use:   "pair",
		Short: "Generate a QR code and passcode for device pairing",
		RunE: func(cmd *cobra.Command, args []string) error {
			fmt.Println("Pairing not yet implemented — see the 'pairing' task in docs/plan.md")
			return nil
		},
	}

	// app stop — stop the running daemon
	stopCmd := &cobra.Command{
		Use:   "stop",
		Short: "Stop the running daemon",
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := config.Load()
			if err != nil {
				return fmt.Errorf("load config: %w", err)
			}
			return daemon.Stop(cfg.DataDir)
		},
	}

	// app devices — list paired devices (stub)
	devicesCmd := &cobra.Command{
		Use:   "devices",
		Short: "List paired devices",
		RunE: func(cmd *cobra.Command, args []string) error {
			fmt.Println("No paired devices. Use 'app pair' to pair a device.")
			return nil
		},
	}

	// app revoke — revoke a paired device (stub)
	revokeCmd := &cobra.Command{
		Use:   "revoke <id>",
		Short: "Revoke a paired device's access",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			fmt.Printf("Device revocation not yet implemented — see the 'pairing' task in docs/plan.md\n")
			return nil
		},
	}

	// app logs — tail daemon logs (stub)
	logsCmd := &cobra.Command{
		Use:   "logs",
		Short: "Tail daemon logs",
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := config.Load()
			if err != nil {
				return fmt.Errorf("load config: %w", err)
			}
			logFile := filepath.Join(cfg.DataDir, "daemon.log")
			if _, err := os.Stat(logFile); err != nil {
				fmt.Println("No log file found. Is the daemon running?")
				return nil
			}
			data, err := os.ReadFile(logFile)
			if err != nil {
				return err
			}
			fmt.Print(string(data))
			return nil
		},
	}

	rootCmd.AddCommand(startCmd, stopCmd, statusCmd, addFolderCmd, pairCmd, devicesCmd, revokeCmd, logsCmd)

	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}
