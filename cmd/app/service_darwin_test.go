//go:build darwin

package main

import (
	"strings"
	"testing"
)

// TestLaunchPlistContent verifies the generated launchd plist is well-formed
// enough to load and references the binary path. Also checks XML escaping of
// special characters in the binary path.
func TestLaunchPlistContent(t *testing.T) {
	binary := "/usr/local/bin/app"
	stdoutLog := "/Users/test/Library/Logs/local-agent.log"
	stderrLog := "/Users/test/Library/Logs/local-agent.err"
	content := launchPlistContent(binary, stdoutLog, stderrLog)

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
		"<key>StandardOutPath</key>",
		"<string>" + stdoutLog + "</string>",
		"<key>StandardErrorPath</key>",
		"<string>" + stderrLog + "</string>",
		"</plist>",
	}
	for _, want := range checks {
		if !strings.Contains(content, want) {
			t.Errorf("launchd plist missing %q\n--- plist ---\n%s", want, content)
		}
	}

	// Verify XML escaping for paths with special characters.
	evil := `/Users/foo/A&B/app`
	escaped := launchPlistContent(evil, stdoutLog, stderrLog)
	if strings.Contains(escaped, "<string>"+evil+"</string>") {
		t.Errorf("plist did not XML-escape & in binary path\n--- plist ---\n%s", escaped)
	}
	if !strings.Contains(escaped, "<string>/Users/foo/A&amp;B/app</string>") {
		t.Errorf("plist missing XML-escaped binary path\n--- plist ---\n%s", escaped)
	}
}
