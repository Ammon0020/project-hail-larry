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

			d := daemon.New(&daemon.Config{
				Port:    cfg.Port,
				Host:    cfg.Host,
				DataDir: cfg.DataDir,
				DBPath:  cfg.DBPath,
			})

			return d.Start(context.Background())
		},
	}

	// app status — show daemon info (stub for now)
	statusCmd := &cobra.Command{
		Use:   "status",
		Short: "Show daemon status",
		RunE: func(cmd *cobra.Command, args []string) error {
			cfg, err := config.Load()
			if err != nil {
				return fmt.Errorf("load config: %w", err)
			}
			fmt.Printf("Host: %s\nPort: %d\nData: %s\n", cfg.Host, cfg.Port, cfg.DataDir)
			fmt.Printf("Workspaces: %d\n", len(cfg.Workspaces))
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

	rootCmd.AddCommand(startCmd, statusCmd, addFolderCmd, pairCmd)

	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}
