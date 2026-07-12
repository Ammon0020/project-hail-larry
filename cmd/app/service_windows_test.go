//go:build windows

package main

import "testing"

// TestRunKeyValue verifies the Windows registry value quotes the binary path
// (so paths with spaces parse correctly) and appends the start subcommand.
func TestRunKeyValue(t *testing.T) {
	cases := []struct {
		binary string
		want   string
	}{
		{`C:\app.exe`, `"C:\app.exe" start`},
		{`C:\Program Files\app.exe`, `"C:\Program Files\app.exe" start`},
	}
	for _, c := range cases {
		got := runKeyValue(c.binary)
		if got != c.want {
			t.Errorf("runKeyValue(%q) = %q, want %q", c.binary, got, c.want)
		}
	}
}
