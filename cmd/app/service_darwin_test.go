//go:build darwin

package main

import (
	"strings"
	"testing"
)

// TestLaunchPlistContent verifies the generated launchd plist is well-formed
// enough to load and references the binary path.
func TestLaunchPlistContent(t *testing.T) {
	binary := "/usr/local/bin/app"
	content := launchPlistContent(binary)

	checks := []string{
		`<?xml version="1.0" encoding="UTF-8"?>`,
		"<plist version=\"1.0\">",
		"<key>Label</key>",
		"<string>com.local-agent</string>",
		"<key>ProgramArguments</key>",
		"<string>" + binary + "</string>",
		"<string>start</string>",
		"<key>RunAtLoad</key>",
		"<true/>",
		"<key>KeepAlive</key>",
		"</plist>",
	}
	for _, want := range checks {
		if !strings.Contains(content, want) {
			t.Errorf("launchd plist missing %q\n--- plist ---\n%s", want, content)
		}
	}
}
