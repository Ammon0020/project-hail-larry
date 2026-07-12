// Package main is the CLI entry point for the Local Agent Interface.
// Uses cobra for command structure. Blueprint references: Sec 4 (Host Daemon).
package main

import (
	"bytes"
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"

	"github.com/adama/local-agent/internal/config"
	"github.com/adama/local-agent/internal/daemon"
	"github.com/adama/local-agent/internal/workspace"
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
		newRemoveFolderCommand(),
		newListFoldersCommand(),
		newPairCommand(),
		newDevicesCommand(),
		newRevokeCommand(),
		newLogsCommand(),
		newInstallServiceCommand(),
		newUninstallServiceCommand(),
	)

	return rootCmd
}

// newInstallServiceCommand registers the daemon as a per-user system service
// that starts on boot (systemd user unit / launchd LaunchAgent / Windows Run-key).
// System-wide installation is not supported — it would require root/admin
// privileges and is intentionally not implemented.
func newInstallServiceCommand() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "install-service",
		Short: "Register the daemon as a user service that starts on boot",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			if err := installService(); err != nil {
				return err
			}
			return writeln(cmd.OutOrStdout(), "Service installed.")
		},
	}
	return cmd
}

// newUninstallServiceCommand removes the system service previously registered
// by install-service.
func newUninstallServiceCommand() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "uninstall-service",
		Short: "Remove the system service registered by install-service",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			if err := uninstallService(); err != nil {
				return err
			}
			return writeln(cmd.OutOrStdout(), "Service uninstalled.")
		},
	}
	return cmd
}

// resolveBinaryPath returns the absolute path to the running executable, used
// when registering the daemon as a system service so the service file points at
// the same binary the user invoked.
func resolveBinaryPath() (string, error) {
	binary, err := os.Executable()
	if err != nil {
		return "", fmt.Errorf("resolve executable path: %w", err)
	}
	return binary, nil
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

	// Check for duplicate workspace registration.
	for _, ws := range cfg.Workspaces {
		if ws == absPath {
			return writef(cmd.OutOrStdout(), "Workspace already registered: %s\n", absPath)
		}
	}

	cfg.Workspaces = append(cfg.Workspaces, absPath)
	if err := cfg.Save(); err != nil {
		return fmt.Errorf("save config: %w", err)
	}

	return writef(cmd.OutOrStdout(), "Workspace registered: %s\n", absPath)
}

func newRemoveFolderCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "remove-folder <id>",
		Short: "Unregister a workspace directory by ID",
		Args:  cobra.ExactArgs(1),
		RunE:  runRemoveFolder,
	}
}

func runRemoveFolder(cmd *cobra.Command, args []string) error {
	cfg, err := loadConfig()
	if err != nil {
		return err
	}

	workspaceID := args[0]

	// Build a workspace manager from the persisted config so we can resolve
	// the ID to a path and call Remove. This mirrors how the daemon loads
	// workspaces on startup.
	wsMgr := workspace.NewManager()
	for _, wsPath := range cfg.Workspaces {
		if _, regErr := wsMgr.Register(context.Background(), wsPath); regErr != nil {
			// Skip paths that no longer exist on disk.
			continue
		}
	}

	// Find the workspace to get its path before removing.
	workspaces, err := wsMgr.List(context.Background())
	if err != nil {
		return fmt.Errorf("list workspaces: %w", err)
	}
	var wsPath string
	for _, ws := range workspaces {
		if ws.ID == workspaceID {
			wsPath = ws.Path
			break
		}
	}
	if wsPath == "" {
		return fmt.Errorf("workspace not found: %s", workspaceID)
	}

	// Remove from the in-memory manager.
	if err := wsMgr.Remove(context.Background(), workspaceID); err != nil {
		return fmt.Errorf("remove workspace: %w", err)
	}

	// Drop the path from persisted config and save.
	if err := cfg.RemoveWorkspacePath(wsPath); err != nil {
		return fmt.Errorf("update config: %w", err)
	}

	return writef(cmd.OutOrStdout(), "Workspace removed: %s (%s)\n", workspaceID, wsPath)
}

func newListFoldersCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "list-folders",
		Short: "List registered workspace directories",
		Args:  cobra.NoArgs,
		RunE:  runListFolders,
	}
}

func runListFolders(cmd *cobra.Command, _ []string) error {
	cfg, err := loadConfig()
	if err != nil {
		return err
	}

	// Build a workspace manager from the persisted config so IDs are
	// resolved consistently (deterministic hash of the path).
	wsMgr := workspace.NewManager()
	for _, wsPath := range cfg.Workspaces {
		if _, regErr := wsMgr.Register(context.Background(), wsPath); regErr != nil {
			continue
		}
	}

	workspaces, err := wsMgr.List(context.Background())
	if err != nil {
		return fmt.Errorf("list workspaces: %w", err)
	}

	if len(workspaces) == 0 {
		return writeln(cmd.OutOrStdout(), "No workspaces registered. Use 'app add-folder <path>' to add one.")
	}

	for _, ws := range workspaces {
		if err := writef(cmd.OutOrStdout(), "%s\t%s\t%s\n", ws.ID, ws.Name, ws.Path); err != nil {
			return err
		}
	}
	return nil
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
	cfg, err := loadConfig()
	if err != nil {
		return err
	}

	// Build the request body with encoding/json so a host containing quotes,
	// backslashes, or control characters cannot produce invalid JSON or escape
	// the JSON string context.
	body, err := json.Marshal(struct {
		Host string `json:"host"`
		Port int    `json:"port"`
	}{Host: pairingHost(cfg.Host), Port: cfg.Port})
	if err != nil {
		return fmt.Errorf("marshal pairing request: %w", err)
	}

	session, err := callAPI[pairingSession](cfg, http.MethodPost, "/api/pair/initiate", bytes.NewReader(body), "pairing failed")
	if err != nil {
		return err
	}
	return writePairingSession(cmd.OutOrStdout(), *session)
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

	devices, err := callAPI[[]pairedDevice](cfg, http.MethodGet, "/api/devices", nil, "list devices failed")
	if err != nil {
		return err
	}
	return writeDevices(cmd.OutOrStdout(), *devices)
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
	cfg, err := loadConfig()
	if err != nil {
		return err
	}

	deviceID := args[0]
	if _, err := callAPI[struct{}](cfg, http.MethodDelete, "/api/devices/"+deviceID, nil, "revoke failed"); err != nil {
		return err
	}
	return writef(cmd.OutOrStdout(), "Device %s revoked.\n", deviceID)
}

func newLogsCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "logs",
		Short: "Print daemon logs (tail of the last 64KB)",
		Args:  cobra.NoArgs,
		RunE:  runLogs,
	}
}

// logTailBytes is the maximum number of trailing bytes streamed from the
// daemon log. Streaming only the tail avoids loading an unbounded multi-MB log
// file into memory, while still showing the most recent activity.
const logTailBytes = 64 * 1024

func runLogs(cmd *cobra.Command, _ []string) error {
	cfg, err := loadConfig()
	if err != nil {
		return err
	}

	logFile := filepath.Join(cfg.DataDir, "daemon.log")
	info, err := os.Stat(logFile)
	if err != nil {
		if os.IsNotExist(err) {
			return writeln(cmd.OutOrStdout(), "No log file found. Is the daemon running?")
		}
		return fmt.Errorf("stat log file: %w", err)
	}

	// Open the file and stream the tail directly to stdout instead of buffering
	// the entire log into memory.
	f, err := os.Open(logFile) //nolint:gosec // logFile is constructed from the app config data directory.
	if err != nil {
		return fmt.Errorf("open log file: %w", err)
	}
	defer func() { _ = f.Close() }()

	// Seek to the last logTailBytes so we only stream the recent tail. If the
	// file is smaller than the tail window, read from the start.
	size := info.Size()
	offset := int64(0)
	if size > logTailBytes {
		offset = size - logTailBytes
	}
	if _, err := f.Seek(offset, io.SeekStart); err != nil {
		return fmt.Errorf("seek log file: %w", err)
	}

	// If we seeked into the middle of the file, skip the partial first line so
	// output starts on a line boundary.
	if offset > 0 {
		buf := make([]byte, 1)
		for {
			if _, err := f.Read(buf); err != nil {
				if err == io.EOF {
					break
				}
				return fmt.Errorf("read log file: %w", err)
			}
			if buf[0] == '\n' {
				break
			}
		}
	}

	if _, err := io.Copy(cmd.OutOrStdout(), f); err != nil {
		return fmt.Errorf("stream log file: %w", err)
	}
	return nil
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
		Port:              cfg.Port,
		Host:              cfg.Host,
		DataDir:           cfg.DataDir,
		DBPath:            cfg.DBPath,
		TLSEnabled:        cfg.TLSEnabled,
		TLSCertDir:        cfg.TLSCertDir,
		HTTPSPort:         cfg.HTTPSPort,
		PairingTTLSeconds: cfg.PairingTTLSeconds,
		// Wire the sliding-TTL device credential inactivity window through to the
		// daemon so CLI-started daemons actually enforce credential expiry.
		CredentialInactivityTTLSeconds: cfg.CredentialInactivityTTLSeconds,
	}
}

func pairingHost(host string) string {
	if host == "0.0.0.0" {
		return localAPIHost
	}
	return host
}

// localAPIURL builds the URL for a local daemon API call, selecting https when
// the daemon is configured to serve TLS so CLI commands work against a
// TLS-enabled daemon (not just plain HTTP).
func localAPIURL(cfg *config.Config, path string) string {
	scheme := "http"
	if cfg.TLSEnabled {
		scheme = "https"
	}
	return fmt.Sprintf("%s://%s:%d%s", scheme, localAPIHost, cfg.Port, path)
}

// localHTTPClient returns an http.Client appropriate for talking to the local
// daemon. When TLS is enabled, it trusts the daemon's self-signed certificate
// (loaded from cfg.TLSCertDir/cert.pem) so the CLI can validate the server. If
// the cert cannot be loaded, it falls back to skipping verification — this is
// only used for localhost CLI calls to the user's own daemon, so the risk of a
// MITM on the loopback interface is acceptable.
func localHTTPClient(cfg *config.Config) *http.Client {
	if !cfg.TLSEnabled {
		return http.DefaultClient
	}

	// Try to trust the daemon's self-signed cert explicitly.
	if certPEM, err := os.ReadFile(filepath.Join(cfg.TLSCertDir, "cert.pem")); err == nil {
		pool := x509.NewCertPool()
		if pool.AppendCertsFromPEM(certPEM) {
			return &http.Client{
				Transport: &http.Transport{
					TLSClientConfig: &tls.Config{RootCAs: pool}, // explicit trust of the user's own daemon cert.
				},
			}
		}
	}

	// Fallback: skip verification for localhost-only CLI usage.
	return &http.Client{
		Transport: &http.Transport{
			TLSClientConfig: &tls.Config{InsecureSkipVerify: true}, //nolint:gosec // localhost-only CLI client; no MITM risk on loopback.
		},
	}
}

func statusError(resp *http.Response, prefix string) error {
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("%s (HTTP %d): read response body: %w", prefix, resp.StatusCode, err)
	}
	return fmt.Errorf("%s (HTTP %d): %s", prefix, resp.StatusCode, string(body))
}

// callAPI makes an HTTP request to the local daemon API and decodes the JSON
// response into a value of type T. It verifies the daemon is running, builds
// the request from method/path/body (setting Content-Type: application/json
// when a body is supplied), and returns a statusError when the response is not
// 200 OK. errMsg prefixes the HTTP-call and status errors.
func callAPI[T any](cfg *config.Config, method, path string, body io.Reader, errMsg string) (*T, error) {
	if err := requireDaemonRunning(cfg.DataDir); err != nil {
		return nil, err
	}
	req, err := http.NewRequest(method, localAPIURL(cfg, path), body)
	if err != nil {
		return nil, fmt.Errorf("create request: %w", err)
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	resp, err := localHTTPClient(cfg).Do(req)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", errMsg, err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return nil, statusError(resp, errMsg)
	}
	var result T
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &result, nil
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
	return truncateID(id, 12)
}

func truncateID(id string, maxLen int) string {
	if len(id) <= maxLen {
		return id
	}
	return id[:maxLen]
}
