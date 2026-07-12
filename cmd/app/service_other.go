//go:build !linux && !darwin && !windows

package main

import "fmt"

// installService is a stub for platforms without a supported autostart
// mechanism. The daemon is cross-platform, but boot-time service registration
// is only implemented for Linux (systemd), macOS (launchd), and Windows
// (Run key).
func installService(user bool) error {
	return fmt.Errorf("install-service is not supported on this platform")
}

// uninstallService mirrors installService for unsupported platforms.
func uninstallService(user bool) error {
	return fmt.Errorf("uninstall-service is not supported on this platform")
}
