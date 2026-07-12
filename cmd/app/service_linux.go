//go:build linux

package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
)

// systemdUnitPath returns the path to the user systemd unit file for the
// local-agent service.
func systemdUnitPath() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("resolve home directory: %w", err)
	}
	return filepath.Join(home, ".config", "systemd", "user", "local-agent.service"), nil
}

// systemdUnitContent builds the systemd user unit file content that runs
// `<binary> start` as a simple service. Extracted for testability.
func systemdUnitContent(binary string) string {
	return fmt.Sprintf(`[Unit]
Description=Local Agent Interface
After=network.target

[Service]
Type=simple
ExecStart=%s start
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
`, binary)
}

// installService registers the daemon as a systemd user service. Only the
// user-level installation is supported; system-wide installation would require
// root and writing under /etc/systemd/system, which is intentionally not
// implemented.
func installService(user bool) error {
	if !user {
		return fmt.Errorf("system-wide install is not supported on Linux; use --user (the default)")
	}

	binary, err := os.Executable()
	if err != nil {
		return fmt.Errorf("resolve executable path: %w", err)
	}

	unitPath, err := systemdUnitPath()
	if err != nil {
		return err
	}

	// Refuse to silently overwrite an existing unit so the user is aware that
	// a service is already registered.
	if _, err := os.Stat(unitPath); err == nil {
		return fmt.Errorf("service unit already exists at %s — run 'app uninstall-service' first", unitPath)
	} else if !os.IsNotExist(err) {
		return fmt.Errorf("stat unit file: %w", err)
	}

	// Create the parent directory (e.g. ~/.config/systemd/user) if needed.
	if err := os.MkdirAll(filepath.Dir(unitPath), 0o750); err != nil {
		return fmt.Errorf("create unit directory: %w", err)
	}

	content := systemdUnitContent(binary)
	//nolint:gosec // unit file is created with mode 0644 (readable, not executable).
	if err := os.WriteFile(unitPath, []byte(content), 0o644); err != nil {
		return fmt.Errorf("write unit file: %w", err)
	}

	// Reload systemd so it picks up the new unit, then enable it so it starts on
	// login. Errors here surface the command output for diagnosis.
	if out, err := exec.Command("systemctl", "--user", "daemon-reload").CombinedOutput(); err != nil {
		return fmt.Errorf("systemctl --user daemon-reload: %w: %s", err, out)
	}
	if out, err := exec.Command("systemctl", "--user", "enable", "local-agent.service").CombinedOutput(); err != nil {
		return fmt.Errorf("systemctl --user enable: %w: %s", err, out)
	}

	fmt.Printf("Installed systemd user unit: %s\n", unitPath)
	fmt.Println("Start it now with: systemctl --user start local-agent.service")
	return nil
}

// uninstallService disables and removes the systemd user service.
func uninstallService(user bool) error {
	if !user {
		return fmt.Errorf("system-wide uninstall is not supported on Linux; use --user (the default)")
	}

	unitPath, err := systemdUnitPath()
	if err != nil {
		return err
	}

	// Best-effort disable; if the unit is already gone, systemctl returns an
	// error which we surface but don't treat as fatal so the file removal still
	// happens.
	if out, err := exec.Command("systemctl", "--user", "disable", "local-agent.service").CombinedOutput(); err != nil {
		fmt.Fprintf(os.Stderr, "warning: systemctl --user disable: %v: %s\n", err, out)
	}

	if err := os.Remove(unitPath); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("remove unit file: %w", err)
	}

	if out, err := exec.Command("systemctl", "--user", "daemon-reload").CombinedOutput(); err != nil {
		return fmt.Errorf("systemctl --user daemon-reload: %w: %s", err, out)
	}

	fmt.Printf("Removed systemd user unit: %s\n", unitPath)
	return nil
}
