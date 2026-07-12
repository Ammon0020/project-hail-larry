package acp

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

// TestExpandPathTilde verifies that a leading ~ is expanded to the user's
// home directory.
func TestExpandPathTilde(t *testing.T) {
	home, err := os.UserHomeDir()
	if err != nil {
		t.Fatalf("os.UserHomeDir: %v", err)
	}

	got := expandPath("~/foo/bar")
	want := filepath.Join(home, "foo", "bar")
	if got != want {
		t.Errorf("expandPath(~/foo/bar) = %q, want %q", got, want)
	}

	// Bare ~ should resolve to the home dir itself.
	if got := expandPath("~"); got != home {
		t.Errorf("expandPath(~) = %q, want %q", got, home)
	}
}

// TestExpandPathWindowsEnv verifies that %VAR% references are replaced with
// the corresponding environment variable values.
func TestExpandPathWindowsEnv(t *testing.T) {
	t.Setenv("LOCALAPPDATA", os.TempDir())

	in := `%LOCALAPPDATA%\Programs\Devin\bin`
	got := expandPath(in)
	// expandWindowsEnv only substitutes %VAR% — it does not normalize path
	// separators — so the backslashes are preserved verbatim on every OS.
	// (Using filepath.Join here would make the expectation OS-dependent and
	// fail on non-Windows platforms.)
	want := os.TempDir() + `\Programs\Devin\bin`
	if got != want {
		t.Errorf("expandPath(%q) = %q, want %q", in, got, want)
	}
}

// TestExpandPathNoVars verifies that a plain path with no variables or ~ is
// returned unchanged.
func TestExpandPathNoVars(t *testing.T) {
	in := `/Applications/Devin.app/Contents/Resources/bin`
	if got := expandPath(in); got != in {
		t.Errorf("expandPath(%q) = %q, want unchanged", in, got)
	}
}

// TestExpandPathUnknownEnv verifies that an undefined %VAR% expands to empty,
// collapsing the reference rather than leaving the literal %VAR% behind.
func TestExpandPathUnknownEnv(t *testing.T) {
	t.Setenv("DEFINITELY_NOT_SET_ACP_TEST", "")
	// Use a var name we just cleared so the lookup returns "".
	got := expandPath(`%DEFINITELY_NOT_SET_ACP_TEST%\sub`)
	if strings.Contains(got, "%DEFINITELY_NOT_SET_ACP_TEST%") {
		t.Errorf("expandPath left literal %%VAR%%: %q", got)
	}
	if !strings.HasSuffix(got, "sub") {
		t.Errorf("expandPath with undefined var = %q, want suffix %q", got, "sub")
	}
}

// TestFindFirstCommandSearchPathFound verifies that findFirstCommand locates a
// binary inside a search path when it is not on PATH.
func TestFindFirstCommandSearchPathFound(t *testing.T) {
	dir := t.TempDir()

	// Pick a command name that is extremely unlikely to already be on PATH.
	cmdName := "acp-test-fake-binary-do-not-exist"
	binPath := filepath.Join(dir, cmdName)
	if runtime.GOOS == "windows" {
		binPath += ".exe"
	}
	if err := os.WriteFile(binPath, []byte("#!/bin/sh\nexit 0\n"), 0o755); err != nil {
		t.Fatalf("write fake binary: %v", err)
	}

	got := findFirstCommand([]string{cmdName}, []string{dir})
	if got == "" {
		t.Fatalf("findFirstCommand returned empty; expected %q", binPath)
	}
	// The returned path should resolve to the file we created (filepath.Join
	// may clean trailing separators, so compare cleaned forms).
	if filepath.Clean(got) != filepath.Clean(binPath) {
		t.Errorf("findFirstCommand = %q, want %q", got, binPath)
	}
}

// TestFindFirstCommandSearchPathFallback verifies that the search path is only
// consulted after exec.LookPath fails, and that a PATH hit wins.
func TestFindFirstCommandSearchPathFallback(t *testing.T) {
	dir := t.TempDir()
	cmdName := "acp-test-fake-binary-fallback"
	binPath := filepath.Join(dir, cmdName)
	if runtime.GOOS == "windows" {
		binPath += ".exe"
	}
	if err := os.WriteFile(binPath, []byte("fake"), 0o755); err != nil {
		t.Fatalf("write fake binary: %v", err)
	}

	// A second, empty search dir that does NOT contain the binary. The first
	// search dir should still produce a hit.
	emptyDir := t.TempDir()
	got := findFirstCommand([]string{cmdName}, []string{emptyDir, dir})
	if got == "" {
		t.Fatalf("findFirstCommand returned empty; expected fallback hit")
	}
	if filepath.Clean(got) != filepath.Clean(binPath) {
		t.Errorf("findFirstCommand fallback = %q, want %q", got, binPath)
	}
}

// TestFindFirstCommandNothingFound verifies that an empty string is returned
// when the binary is neither on PATH nor in any search path.
func TestFindFirstCommandNothingFound(t *testing.T) {
	cmdName := "acp-test-fake-binary-not-present"
	dir := t.TempDir()

	got := findFirstCommand([]string{cmdName}, []string{dir})
	if got != "" {
		t.Errorf("findFirstCommand = %q, want empty string", got)
	}
}

// TestFindFirstCommandWindowsExtension verifies that on Windows the .cmd
// variant is found when the bare name is absent.
func TestFindFirstCommandWindowsExtension(t *testing.T) {
	if runtime.GOOS != "windows" {
		t.Skip("windows-only extension behavior")
	}
	dir := t.TempDir()
	cmdName := "acp-test-fake-cmd"
	cmdPath := filepath.Join(dir, cmdName+".cmd")
	if err := os.WriteFile(cmdPath, []byte("@echo off\r\n"), 0o755); err != nil {
		t.Fatalf("write fake .cmd: %v", err)
	}

	got := findFirstCommand([]string{cmdName}, []string{dir})
	if got == "" {
		t.Fatalf("findFirstCommand returned empty; expected %q", cmdPath)
	}
	if filepath.Clean(got) != filepath.Clean(cmdPath) {
		t.Errorf("findFirstCommand = %q, want %q", got, cmdPath)
	}
}

// TestExpandWindowsEnvReplacement is a direct unit test for the %VAR% regex
// replacement helper.
func TestExpandWindowsEnvReplacement(t *testing.T) {
	t.Setenv("MY_ACP_TEST_VAR", "hello")
	got := expandWindowsEnv("prefix-%MY_ACP_TEST_VAR%-suffix")
	if got != "prefix-hello-suffix" {
		t.Errorf("expandWindowsEnv = %q, want %q", got, "prefix-hello-suffix")
	}

	// Multiple references in one string.
	got = expandWindowsEnv("%MY_ACP_TEST_VAR%/%MY_ACP_TEST_VAR%")
	if got != "hello/hello" {
		t.Errorf("expandWindowsEnv repeated = %q, want %q", got, "hello/hello")
	}

	// No vars present -> unchanged.
	got = expandWindowsEnv("plain-path")
	if got != "plain-path" {
		t.Errorf("expandWindowsEnv plain = %q, want unchanged", got)
	}

	// Undefined var -> empty replacement.
	got = expandWindowsEnv("a-%NOPE_NOT_SET_ACP%/b")
	if !strings.HasSuffix(got, "/b") || !strings.HasPrefix(got, "a-") {
		t.Errorf("expandWindowsEnv undefined = %q, want a-/b", got)
	}
}

// TestLooksLikeRawID verifies that opaque tool-call identifiers are recognized
// as IDs (so they fall back to a kind label) while human-readable titles are
// preserved. Guards the "Permission Required / <random-id>" regression.
func TestLooksLikeRawID(t *testing.T) {
	ids := []string{
		"muNNhDHjd",                            // classic short random token
		"toolu_01HABCDEF0123456789",            // Claude tool-use ID
		"call_abc123XYZ",                       // OpenAI call ID
		"fc_9f8e7d",                            // function-call ID
		"550e8400-e29b-41d4-a716-446655440000", // UUID with hyphens
		"550e8400e29b41d4a716446655440000",     // UUID without hyphens
		"deadbeefcafebabe1234",                 // long hex token
		"",                                     // empty -> treat as ID (no label)
		"   ",                                  // whitespace only
	}
	for _, s := range ids {
		if !looksLikeRawID(s) {
			t.Errorf("looksLikeRawID(%q) = false, want true (should be treated as an ID)", s)
		}
	}

	labels := []string{
		"Run command",            // synthesized kind label
		"Edit file",              // synthesized kind label
		"Read package.json",      // multi-word, has space
		"read_file",              // snake_case tool name (not an ID prefix)
		"mcp__server__do_thing",  // MCP-style tool name
		"call_my_helper_routine", // descriptive name with extra separators
		"Search the codebase",    // multi-word
	}
	for _, s := range labels {
		if looksLikeRawID(s) {
			t.Errorf("looksLikeRawID(%q) = true, want false (should be treated as a real label)", s)
		}
	}
}

// TestCodexSpecExcludesBareTUI verifies that the codex agent spec in
// knownAgents does NOT include the bare "codex" command. The bare "codex"
// binary is the OpenAI Codex CLI — an interactive TUI that requires a TTY on
// stdin. Spawning it over pipes (as the ACP transport does) makes it exit
// immediately with "stdin is not a terminal", causing the ACP Initialize
// handshake to fail with "peer disconnected before response". Only the
// separate "codex-acp" adapter package speaks ACP over stdio.
//
// This is a static regression guard — it always runs, no agent installation
// required.
func TestCodexSpecExcludesBareTUI(t *testing.T) {
	for _, spec := range knownAgents {
		if spec.id != "codex" {
			continue
		}
		for _, cmd := range spec.commands {
			if cmd == "codex" {
				t.Fatalf("codex agent spec includes bare %q command — the OpenAI Codex CLI is a TUI "+
					"that cannot speak ACP over stdio. Only %q should be listed.", cmd, "codex-acp")
			}
		}
		if len(spec.commands) == 0 {
			t.Fatal("codex agent spec has no commands")
		}
		// Confirm codex-acp is the command we expect.
		foundACP := false
		for _, cmd := range spec.commands {
			if cmd == "codex-acp" {
				foundACP = true
			}
		}
		if !foundACP {
			t.Fatalf("codex agent spec does not include %q: commands=%v", "codex-acp", spec.commands)
		}
		return
	}
	t.Fatal("codex agent spec not found in knownAgents")
}

// codexInstalled reports whether the bare "codex" CLI is on PATH. Used to
// gate integration tests that verify the TUI-fallback regression doesn't
// recur.
func codexInstalled() bool {
	_, err := exec.LookPath("codex")
	return err == nil
}

// TestCodexTUINotACPCompatible is an integration test that proves the bare
// "codex" CLI is a TUI that cannot be used as an ACP agent. It spawns "codex"
// with no args over pipes (exactly as the ACP transport would) and verifies
// it exits with a "stdin is not a terminal" error rather than speaking ACP.
//
// This documents the root cause of the "peer disconnected before response"
// bug: the autodetect used to fall back from "codex-acp" to the bare "codex"
// TUI, which can never work.
//
// Only runs when the "codex" CLI is installed.
func TestCodexTUINotACPCompatible(t *testing.T) {
	if !codexInstalled() {
		t.Skip("codex CLI not installed — skipping TUI compatibility test")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, "codex")
	// Use pipes, not a TTY — this is what the ACP transport does.
	stdin, err := cmd.StdinPipe()
	if err != nil {
		t.Fatalf("stdin pipe: %v", err)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatalf("stdout pipe: %v", err)
	}
	var stderr strings.Builder
	cmd.Stderr = &stderr

	if err := cmd.Start(); err != nil {
		t.Fatalf("start codex: %v", err)
	}

	// The TUI should exit quickly with "stdin is not a terminal".
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()

	select {
	case <-time.After(8 * time.Second):
		_ = cmd.Process.Kill()
		t.Fatal("codex did not exit within 8s — expected it to refuse non-TTY stdin")
	case waitErr := <-done:
		// The process should have exited with a non-zero status and a clear
		// stderr message about stdin not being a terminal.
		_ = stdin.Close()
		_ = stdout.Close()

		stderrText := stderr.String()
		t.Logf("codex exited: err=%v stderr=%q", waitErr, stderrText)

		if !strings.Contains(strings.ToLower(stderrText), "stdin is not a terminal") {
			t.Fatalf("codex stderr does not mention 'stdin is not a terminal'; "+
				"the TUI behavior may have changed. stderr: %s", stderrText)
		}
	}
}

// TestCodexAutodetectNoTUIFallback is an integration test that verifies
// Autodetect() does not register a codex agent whose Command points to the
// bare "codex" TUI. Before the fix, when "codex-acp" was absent but "codex"
// was on PATH, autodetect fell back to the TUI — causing every session to
// fail with "peer disconnected before response".
//
// Only runs when the "codex" CLI is installed.
func TestCodexAutodetectNoTUIFallback(t *testing.T) {
	if !codexInstalled() {
		t.Skip("codex CLI not installed — skipping autodetect fallback test")
	}

	codexACPPath, codexACPErr := exec.LookPath("codex-acp")
	codexPath, _ := exec.LookPath("codex")
	codexACPInstalled := codexACPErr == nil

	agents := Autodetect()

	var codexAgent *AgentInfo
	for i := range agents {
		if agents[i].ID == "codex" {
			codexAgent = &agents[i]
			break
		}
	}

	if !codexACPInstalled {
		// codex-acp is not installed, so the codex agent should NOT be
		// detected at all — the bare TUI is not a valid ACP agent.
		if codexAgent != nil {
			t.Fatalf("codex agent was detected without codex-acp installed; "+
				"Command=%q (codex=%q). The bare codex TUI cannot speak ACP and "+
				"should not be registered.", codexAgent.Command, codexPath)
		}
		t.Logf("correct: codex not detected (codex-acp not installed, codex TUI at %q is not ACP-compatible)", codexPath)
		return
	}

	// codex-acp IS installed — the codex agent should be detected, and its
	// Command must point to codex-acp, not the bare codex TUI.
	if codexAgent == nil {
		t.Fatal("codex-acp is installed but codex agent was not detected")
	}
	if codexAgent.Command == codexPath && codexPath != codexACPPath {
		t.Fatalf("codex agent Command points to the bare TUI (%q), not codex-acp (%q)",
			codexAgent.Command, codexACPPath)
	}
	t.Logf("correct: codex agent detected with Command=%q", codexAgent.Command)
}

func TestIsProvidersListUnsupported(t *testing.T) {
	cases := []struct {
		in   string
		want bool
	}{
		{"", false},
		{`providers/list failed: {"code":-32601,"message":"Method not found"}`, true},
		{"providers/list not supported", true},
		{"initialize failed: peer disconnected", false},
		{"METHOD NOT FOUND", true},
	}
	for _, tc := range cases {
		if got := isProvidersListUnsupported(tc.in); got != tc.want {
			t.Errorf("isProvidersListUnsupported(%q)=%v want %v", tc.in, got, tc.want)
		}
	}
}

func TestTruncateDiag(t *testing.T) {
	if got := truncateDiag("  hello  ", 100); got != "hello" {
		t.Errorf("truncateDiag trim = %q", got)
	}
	long := strings.Repeat("a", 50)
	got := truncateDiag(long, 10)
	if !strings.HasPrefix(got, "aaaaaaaaaa") || !strings.HasSuffix(got, "…") {
		t.Errorf("truncateDiag long = %q", got)
	}
}

func TestStripANSI(t *testing.T) {
	in := "\x1b[2K\x1b[Gauto - Auto"
	if got := stripANSI(in); got != "auto - Auto" {
		t.Errorf("stripANSI = %q", got)
	}
}
