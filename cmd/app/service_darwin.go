//go:build darwin

package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// launchAgentPath returns the path to the per-user launchd plist for the
// local-agent service.
func launchAgentPath() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("resolve home directory: %w", err)
	}
	return filepath.Join(home, "Library", "LaunchAgents", "com.local-agent.plist"), nil
}

// xmlEscape escapes the characters that are special in XML attribute values
// and text content (&, <, >, "). This prevents malformed plists when the
// binary path contains characters like & or <.
func xmlEscape(s string) string {
	return strings.NewReplacer(
		"&", "&amp;",
		"<", "&lt;",
		">", "&gt;",
		`"`, "&quot;",
	).Replace(s)
}

// logPath returns a user-private log file path under ~/Library/Logs. Using a
// user-owned directory instead of /tmp avoids symlink attacks where a local
// attacker pre-creates /tmp/local-agent.log as a symlink to a sensitive file.
func logPath(filename string) (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("resolve home directory: %w", err)
	}
	return filepath.Join(home, "Library", "Logs", filename), nil
}

// launchPlistContent builds the launchd LaunchAgent plist that runs
// `<binary> start` at login, keeping it alive on crash. The binary path is
// XML-escaped to prevent malformed plists when it contains & < > or ".
// Logs go to ~/Library/Logs (user-private) not /tmp (world-writable).
// Extracted for testability.
func launchPlistContent(binary, stdoutLog, stderrLog string) string {
	escaped := xmlEscape(binary)
	return fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.local-agent</string>
    <key>ProgramArguments</key>
    <array>
        <string>%s</string>
        <string>start</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>%s</string>
    <key>StandardErrorPath</key>
    <string>%s</string>
</dict>
</plist>
`, escaped, xmlEscape(stdoutLog), xmlEscape(stderrLog))
}

// installService registers the daemon as a launchd LaunchAgent that runs at
// login. Only the user-level installation is supported.
func installService(user bool) error {
	if !user {
		return fmt.Errorf("system-wide install is not supported on macOS; use --user (the default)")
	}

	binary, err := os.Executable()
	if err != nil {
		return fmt.Errorf("resolve executable path: %w", err)
	}

	plistPath, err := launchAgentPath()
	if err != nil {
		return err
	}

	if _, err := os.Stat(plistPath); err == nil {
		return fmt.Errorf("launch agent already exists at %s — run 'app uninstall-service' first", plistPath)
	} else if !os.IsNotExist(err) {
		return fmt.Errorf("stat plist: %w", err)
	}

	if err := os.MkdirAll(filepath.Dir(plistPath), 0o755); err != nil {
		return fmt.Errorf("create LaunchAgents directory: %w", err)
	}

	stdoutLog, err := logPath("local-agent.log")
	if err != nil {
		return err
	}
	stderrLog, err := logPath("local-agent.err")
	if err != nil {
		return err
	}

	content := launchPlistContent(binary, stdoutLog, stderrLog)
	//nolint:gosec // plist is a config file, mode 0644.
	if err := os.WriteFile(plistPath, []byte(content), 0o644); err != nil {
		return fmt.Errorf("write plist: %w", err)
	}

	if out, err := exec.Command("launchctl", "load", plistPath).CombinedOutput(); err != nil {
		return fmt.Errorf("launchctl load: %w: %s", err, out)
	}

	fmt.Printf("Installed launchd LaunchAgent: %s\n", plistPath)
	fmt.Println("It will start at next login (or run: launchctl start com.local-agent)")
	return nil
}

// uninstallService unloads and removes the launchd LaunchAgent.
func uninstallService(user bool) error {
	if !user {
		return fmt.Errorf("system-wide uninstall is not supported on macOS; use --user (the default)")
	}

	plistPath, err := launchAgentPath()
	if err != nil {
		return err
	}

	// Best-effort unload; surface warnings but proceed to remove the file.
	if out, err := exec.Command("launchctl", "unload", plistPath).CombinedOutput(); err != nil {
		fmt.Fprintf(os.Stderr, "warning: launchctl unload: %v: %s\n", err, out)
	}

	if err := os.Remove(plistPath); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("remove plist: %w", err)
	}

	fmt.Printf("Removed launchd LaunchAgent: %s\n", plistPath)
	return nil
}
