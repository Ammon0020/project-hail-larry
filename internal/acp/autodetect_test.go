package acp

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
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
	want := filepath.Join(os.TempDir(), "Programs", "Devin", "bin")
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
