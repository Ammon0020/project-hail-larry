//go:build windows

package main

import (
	"fmt"
	"strings"

	"golang.org/x/sys/windows/registry"
)

// runKeyName is the registry value name under HKCU\...\Run used to autostart
// the daemon at login.
const runKeyName = "LocalAgent"

// runKeyPath is the per-user Run key under HKEY_CURRENT_USER. A value here
// starts the referenced program at login without requiring admin privileges.
const runKeyPath = `Software\Microsoft\Windows\CurrentVersion\Run`

// runKeyValue builds the registry value that launches `<binary> start` at
// login. The binary path is quoted so paths containing spaces are handled
// correctly by the Windows shell. Extracted for testability.
func runKeyValue(binary string) string {
	// Quote the executable path so a path with spaces (e.g.
	// "C:\Program Files\app.exe") is parsed as a single argument.
	return fmt.Sprintf(`"%s" start`, binary)
}

// installService registers the daemon to start at login via the per-user Run
// registry key. This is the simplest reliable user-level autostart on Windows
// and avoids the admin privileges a true Windows Service would require.
func installService(user bool) error {
	if !user {
		return fmt.Errorf("system-wide install is not supported on Windows; use --user (the default)")
	}

	binary, err := resolveBinaryPath()
	if err != nil {
		return err
	}

	// Open the user Run key for writing. HKCU does not require admin rights.
	key, err := registry.OpenKey(registry.CURRENT_USER, runKeyPath, registry.SET_VALUE|registry.QUERY_VALUE)
	if err != nil {
		return fmt.Errorf("open HKCU\\%s: %w", runKeyPath, err)
	}
	defer func() { _ = key.Close() }()

	// Refuse to silently overwrite an existing entry.
	if existing, _, err := key.GetStringValue(runKeyName); err == nil && existing != "" {
		return fmt.Errorf("autostart entry already exists (HKCU\\%s\\%s = %q) — run 'app uninstall-service' first",
			runKeyPath, runKeyName, existing)
	} else if err != nil && err != registry.ErrNotExist {
		// Some other read error; surface it rather than guessing.
		return fmt.Errorf("read existing run key: %w", err)
	}

	value := runKeyValue(binary)
	if err := key.SetStringValue(runKeyName, value); err != nil {
		return fmt.Errorf("set run key value: %w", err)
	}

	fmt.Printf("Installed autostart entry: HKCU\\%s\\%s = %s\n", runKeyPath, runKeyName, value)
	return nil
}

// uninstallService removes the per-user Run registry entry created by
// installService.
func uninstallService(user bool) error {
	if !user {
		return fmt.Errorf("system-wide uninstall is not supported on Windows; use --user (the default)")
	}

	key, err := registry.OpenKey(registry.CURRENT_USER, runKeyPath, registry.SET_VALUE|registry.QUERY_VALUE)
	if err != nil {
		return fmt.Errorf("open HKCU\\%s: %w", runKeyPath, err)
	}
	defer func() { _ = key.Close() }()

	if err := key.DeleteValue(runKeyName); err != nil {
		if strings.Contains(err.Error(), "RegDeleteValue") {
			// The value did not exist; treat as success so uninstall is idempotent.
			fmt.Println("Autostart entry was not present; nothing to remove.")
			return nil
		}
		return fmt.Errorf("delete run key value: %w", err)
	}

	fmt.Printf("Removed autostart entry: HKCU\\%s\\%s\n", runKeyPath, runKeyName)
	return nil
}
