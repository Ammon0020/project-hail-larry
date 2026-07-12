//go:build linux

package main

import (
	"strings"
	"testing"
)

// TestSystemdUnitContent verifies the generated systemd user unit contains the
// required sections and embeds the binary path verbatim (no hardcoding).
func TestSystemdUnitContent(t *testing.T) {
	binary := "/usr/local/bin/app"
	content := systemdUnitContent(binary)

	checks := []string{
		"[Unit]",
		"Description=Local Agent Interface",
		"After=network.target",
		"[Service]",
		"Type=simple",
		"ExecStart=" + binary + " start",
		"Restart=on-failure",
		"RestartSec=5",
		"[Install]",
		"WantedBy=default.target",
	}
	for _, want := range checks {
		if !strings.Contains(content, want) {
			t.Errorf("systemd unit missing %q\n--- unit ---\n%s", want, content)
		}
	}
}
