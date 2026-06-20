// Package main is the CLI entry point for the Local Agent Interface.
// Uses cobra for command structure. Blueprint references: Sec 4 (Host Daemon).
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"

	"github.com/adama/local-agent/internal/config"
	"github.com/adama/local-agent/internal/daemon"
	"github.com/spf13/cobra"
)

const localAPIHost = "localhost"

type pairingSession struct {
	ID        string `json:"id"`
	Passcode  string `json:"passcode"`
	URL       string `json:"url"`
	QRPath    string `json:"qrPath"`
	ExpiresAt string `json:"expiresAt"`
}

type pairedDevice struct {
	ID       string `json:"id"`
	Name     string `json:"name"`
	PairedAt string `json:"pairedAt"`
}

func main() {
	rootCmd := newRootCommand()
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func newRootCommand() *cobra.Command {
	rootCmd := &cobra.Command{
		Use:   "app",
		Short: "Local Agent Interface — self-hosted AI code editor",
		Long: "A Go daemon that serves a browser-based IDE to devices on your local network. " +
			"Pair devices, orchestrate AI agents via ACP, and edit code from anywhere on your LAN.",
		SilenceUsage:  true,
		SilenceErrors: true,
	}

	rootCmd.AddCommand(
		newStartCommand(),
		newStopCommand(),
		newStatusCommand(),
		newAddFolderCommand(),
		newPairCommand(),
		newDevicesCommand(),
		newRevokeCommand(),
		newLogsCommand(),
	)

	return rootCmd
}

func newStartCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "start",
		Short: "Start the Local Agent Interface daemon",
		Args:  cobra.NoArgs,
		RunE:  runStart,
	}
}

func runStart(_ *cobra.Command, _ []string) error {
	cfg, err := loadConfig()
	if err != nil {
		return err
	}

	d, err := daemon.New(toDaemonConfig(cfg))
	if err != nil {
		return fmt.Errorf("init daemon: %w", err)
	}

	return d.Start(context.Background())
}

func newStatusCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "status",
		Short: "Show daemon status",
		Args:  cobra.NoArgs,
		RunE:  runStatus,
	}
}

func runStatus(cmd *cobra.Command, _ []string) error {
	cfg, err := loadConfig()
	if err != nil {
		return err
	}

	pid, err := daemon.IsRunning(cfg.DataDir)
	if err != nil {
		return fmt.Errorf("check daemon: %w", err)
	}

	return writeStatus(cmd.OutOrStdout(), cfg, pid)
}

func newAddFolderCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "add-folder [path]",
		Short: "Register a workspace directory",
		Args:  cobra.ExactArgs(1),
		RunE:  runAddFolder,
	}
}

func runAddFolder(cmd *cobra.Command, args []string) error {
	cfg, err := loadConfig()
	if err != nil {
		return err
	}

	absPath, err := filepath.Abs(args[0])
	if err != nil {
		return fmt.Errorf("resolve workspace path: %w", err)
	}

	cfg.Workspaces = append(cfg.Workspaces, absPath)
	if err := cfg.Save(); err != nil {
		return fmt.Errorf("save config: %w", err)
	}

	return writef(cmd.OutOrStdout(), "Workspace registered: %s\n", absPath)
}

func newPairCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "pair",
		Short: "Generate a QR code and passcode for device pairing",
		Args:  cobra.NoArgs,
		RunE:  runPair,
	}
}

func runPair(cmd *cobra.Command, _ []string) error {
	cfg, err := loadRunningConfig()
	if err != nil {
		return err
	}

	body := fmt.Sprintf(`{"host":"%s","port":%d}`, pairingHost(cfg.Host), cfg.Port)
	resp, err := http.Post(localAPIURL(cfg.Port, "/api/pair/initiate"), "application/json", strings.NewReader(body))
	if err != nil {
		return fmt.Errorf("call pairing API: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		return statusError(resp, "pairing failed")
	}

	var session pairingSession
	if err := json.NewDecoder(resp.Body).Decode(&session); err != nil {
		return fmt.Errorf("decode pairing response: %w", err)
	}

	return writePairingSession(cmd.OutOrStdout(), session)
}

func newStopCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "stop",
		Short: "Stop the running daemon",
		Args:  cobra.NoArgs,
		RunE:  runStop,
	}
}

func runStop(_ *cobra.Command, _ []string) error {
	cfg, err := loadConfig()
	if err != nil {
		return err
	}
	return daemon.Stop(cfg.DataDir)
}

func newDevicesCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "devices",
		Short: "List paired devices",
		Args:  cobra.NoArgs,
		RunE:  runDevices,
	}
}

func runDevices(cmd *cobra.Command, _ []string) error {
	cfg, err := loadConfig()
	if err != nil {
		return err
	}

	pid, err := daemon.IsRunning(cfg.DataDir)
	if err != nil {
		return fmt.Errorf("check daemon: %w", err)
	}
	if pid == 0 {
		return writeln(cmd.OutOrStdout(), "Daemon is not running. Start it with 'app start'.")
	}

	resp, err := http.Get(localAPIURL(cfg.Port, "/api/devices"))
	if err != nil {
		return fmt.Errorf("call devices API: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		return statusError(resp, "list devices failed")
	}

	var devices []pairedDevice
	if err := json.NewDecoder(resp.Body).Decode(&devices); err != nil {
		return fmt.Errorf("decode devices response: %w", err)
	}

	return writeDevices(cmd.OutOrStdout(), devices)
}

func newRevokeCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "revoke <id>",
		Short: "Revoke a paired device's access",
		Args:  cobra.ExactArgs(1),
		RunE:  runRevoke,
	}
}

func runRevoke(cmd *cobra.Command, args []string) error {
	cfg, err := loadRunningConfig()
	if err != nil {
		return err
	}

	deviceID := args[0]
	req, err := http.NewRequest(http.MethodDelete, localAPIURL(cfg.Port, "/api/devices/"+deviceID), nil)
	if err != nil {
		return fmt.Errorf("create request: %w", err)
	}

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return fmt.Errorf("call revoke API: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		return statusError(resp, "revoke failed")
	}

	return writef(cmd.OutOrStdout(), "Device %s revoked.\n", deviceID)
}

func newLogsCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "logs",
		Short: "Tail daemon logs",
		Args:  cobra.NoArgs,
		RunE:  runLogs,
	}
}

func runLogs(cmd *cobra.Command, _ []string) error {
	cfg, err := loadConfig()
	if err != nil {
		return err
	}

	logFile := filepath.Join(cfg.DataDir, "daemon.log")
	if _, statErr := os.Stat(logFile); statErr != nil {
		if os.IsNotExist(statErr) {
			return writeln(cmd.OutOrStdout(), "No log file found. Is the daemon running?")
		}
		return fmt.Errorf("stat log file: %w", statErr)
	}

	data, err := os.ReadFile(logFile) //nolint:gosec // logFile is constructed from the app config data directory.
	if err != nil {
		return fmt.Errorf("read log file: %w", err)
	}
	return writeString(cmd.OutOrStdout(), string(data))
}

func loadConfig() (*config.Config, error) {
	cfg, err := config.Load()
	if err != nil {
		return nil, fmt.Errorf("load config: %w", err)
	}
	return cfg, nil
}

func loadRunningConfig() (*config.Config, error) {
	cfg, err := loadConfig()
	if err != nil {
		return nil, err
	}
	if err := requireDaemonRunning(cfg.DataDir); err != nil {
		return nil, err
	}
	return cfg, nil
}

func requireDaemonRunning(dataDir string) error {
	pid, err := daemon.IsRunning(dataDir)
	if err != nil {
		return fmt.Errorf("check daemon: %w", err)
	}
	if pid == 0 {
		return fmt.Errorf("daemon is not running — start it with 'app start' first")
	}
	return nil
}

func toDaemonConfig(cfg *config.Config) *daemon.Config {
	return &daemon.Config{
		Port:    cfg.Port,
		Host:    cfg.Host,
		DataDir: cfg.DataDir,
		DBPath:  cfg.DBPath,
	}
}

func pairingHost(host string) string {
	if host == "0.0.0.0" {
		return localAPIHost
	}
	return host
}

func localAPIURL(port int, path string) string {
	return fmt.Sprintf("http://%s:%d%s", localAPIHost, port, path)
}

func statusError(resp *http.Response, prefix string) error {
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("%s (HTTP %d): read response body: %w", prefix, resp.StatusCode, err)
	}
	return fmt.Errorf("%s (HTTP %d): %s", prefix, resp.StatusCode, string(body))
}

func writeStatus(w io.Writer, cfg *config.Config, pid int) error {
	if pid > 0 {
		if err := writef(w, "Status:   Running (PID %d)\n", pid); err != nil {
			return err
		}
	} else if err := writeln(w, "Status:   Stopped"); err != nil {
		return err
	}

	lines := []struct {
		label string
		value any
	}{
		{label: "Host", value: cfg.Host},
		{label: "Port", value: cfg.Port},
		{label: "Data", value: cfg.DataDir},
		{label: "Workspaces", value: len(cfg.Workspaces)},
	}
	for _, line := range lines {
		if err := writef(w, "%-10s %v\n", line.label+":", line.value); err != nil {
			return err
		}
	}
	for _, ws := range cfg.Workspaces {
		if err := writef(w, "  - %s\n", ws); err != nil {
			return err
		}
	}
	return nil
}

func writePairingSession(w io.Writer, session pairingSession) error {
	lines := []string{
		"╔══════════════════════════════════════════════════════╗",
		"║           Device Pairing — Local Agent               ║",
		"╠══════════════════════════════════════════════════════╣",
	}
	for _, line := range lines {
		if err := writeln(w, line); err != nil {
			return err
		}
	}

	fields := []struct {
		label string
		value string
	}{
		{label: "Passcode", value: session.Passcode},
		{label: "URL", value: session.URL},
		{label: "QR Code", value: session.QRPath},
		{label: "Expires", value: session.ExpiresAt},
	}
	for _, field := range fields {
		if err := writef(w, "║  %-9s %-42s║\n", field.label+":", field.value); err != nil {
			return err
		}
	}

	return writeString(w, "╚══════════════════════════════════════════════════════╝\n\n"+
		"Scan the QR code or enter the passcode on your device.\n"+
		"The passcode expires in 5 minutes and can be used once.\n")
}

func writeDevices(w io.Writer, devices []pairedDevice) error {
	if len(devices) == 0 {
		return writeln(w, "No paired devices. Use 'app pair' to pair a device.")
	}

	if err := writef(w, "%-20s %-20s %s\n", "DEVICE ID", "NAME", "PAIRED AT"); err != nil {
		return err
	}
	if err := writeln(w, strings.Repeat("-", 60)); err != nil {
		return err
	}
	for _, d := range devices {
		if err := writef(w, "%-20s %-20s %s\n", shortID(d.ID), d.Name, d.PairedAt); err != nil {
			return err
		}
	}
	return nil
}

func writef(w io.Writer, format string, args ...any) error {
	_, err := fmt.Fprintf(w, format, args...)
	return err
}

func writeln(w io.Writer, args ...any) error {
	_, err := fmt.Fprintln(w, args...)
	return err
}

func writeString(w io.Writer, s string) error {
	_, err := io.WriteString(w, s)
	return err
}

func shortID(id string) string {
	if len(id) <= 12 {
		return id
	}
	return id[:12]
}
