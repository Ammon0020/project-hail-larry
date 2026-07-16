// Package main: CLI command fixture capture.
//
// Each CLI command is run as a subprocess of the real `app` binary built from
// cmd/app, with LOCAL_AGENT_STATE_DIR pointing at the harness's isolated state
// dir. The httptest server is left running so commands that talk to the daemon
// (pair, devices, revoke, logs) hit the in-process server: the harness writes
// a config.json whose Port matches the httptest server's port and a PID file
// pointing at the harness process so daemon.IsRunning reports "running".
//
// Captured per command: stdout, stderr, and exit code, written to
// golden/cli/<command>.txt as a small envelope:
//
//	$ app <args...>
//	exit: <code>
//	--- stdout ---
//	<stdout>
//	--- stderr ---
//	<stderr>
//
// All captured text is redacted (secrets + absolute paths) before writing.
package main

import (
	"bytes"
	"fmt"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// cliCase describes one CLI invocation to capture.
type cliCase struct {
	// name is the golden filename (without extension).
	name string
	// args is the argument vector passed to the app binary (excluding "app").
	args []string
	// skip means the harness should not run this case (e.g. a command that
	// would block or modify the system). When true, only a placeholder
	// fixture explaining the skip is written.
	skip string
}

// captureCLI builds the app binary, prepares the daemon PID file + port config
// so API-talking commands find the in-process server, runs each CLI case, and
// writes golden/cli/<name>.txt.
func captureCLI(h *harness, goldenDir, repoRoot string) error {
	outDir := filepath.Join(goldenDir, "cli")
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		return fmt.Errorf("mkdir %s: %w", outDir, err)
	}

	binPath := filepath.Join(os.TempDir(), "local-agent-contract-app")
	if err := buildApp(repoRoot, binPath); err != nil {
		return err
	}

	// Point the CLI at the in-process httptest server: rewrite config.json
	// with the server's port and write a PID file so requireDaemonRunning
	// passes. The CLI uses localhost + cfg.Port + HTTP (TLSEnabled=false).
	port := httptestPort(h.httpSrv.URL)
	if err := rewriteConfigForCLI(h, port); err != nil {
		return err
	}
	if err := writePIDFile(h); err != nil {
		return err
	}

	// Pair a device through the live server so `app devices` and `app revoke`
	// have a real target. The issued passcode/secret are registered with the
	// redactor so the captured CLI output is scrubbed.
	paired := pairDeviceForCLI(h)

	cases := buildCLICases(paired)
	for _, c := range cases {
		out, err := runCLICase(h, binPath, c)
		if err != nil {
			return fmt.Errorf("cli case %s: %w", c.name, err)
		}
		path := filepath.Join(outDir, c.name+".txt")
		if err := os.WriteFile(path, []byte(out), 0o644); err != nil {
			return fmt.Errorf("write %s: %w", c.name, err)
		}
	}
	return nil
}

// buildCLICases enumerates the CLI commands with stable, contract-relevant
// scenarios. Commands that would block (start) or modify the host system
// (install-service, uninstall-service) are captured as their --help output
// instead, which is itself a stable contract surface.
func buildCLICases(paired *cliPairedDevice) []cliCase {
	return []cliCase{
		{name: "root_help", args: []string{"--help"}},
		{name: "status", args: []string{"status"}},
		{name: "add_folder", args: []string{"add-folder", "tests/contract/fixtures/seed-workspace"}},
		{name: "list_folders", args: []string{"list-folders"}},
		// remove-folder needs a real workspace ID; we capture the
		// "not found" error for a bogus ID instead so the fixture is
		// self-contained and does not depend on add_folder's run order.
		{name: "remove_folder_not_found", args: []string{"remove-folder", "nonexistent-id"}},
		{name: "pair", args: []string{"pair"}},
		{name: "devices", args: []string{"devices"}},
		// revoke the paired device so the command has a real target; the
		// device ID is redacted in the captured output.
		{name: "revoke", args: []string{"revoke", paired.deviceID}},
		{name: "logs", args: []string{"logs"}},
		// stop would SIGTERM the harness's own PID (the PID file points at
		// us); capture the not-running error instead by removing the PID
		// file for this one invocation.
		{name: "stop_not_running", args: []string{"stop"}},
		// start blocks forever; capture --help instead.
		{name: "start_help", args: []string{"start", "--help"}},
		{name: "install_service_help", args: []string{"install-service", "--help"}},
		{name: "uninstall_service_help", args: []string{"uninstall-service", "--help"}},
	}
}

// runCLICase executes one CLI command and returns the redacted envelope text.
func runCLICase(h *harness, binPath string, c cliCase) (string, error) {
	// The "stop_not_running" case must NOT see a PID file, otherwise it would
	// SIGTERM the harness. Remove it for this case only.
	if c.name == "stop_not_running" {
		_ = os.Remove(filepath.Join(h.stateDir, "daemon.pid"))
	}

	cmd := exec.Command(binPath, c.args...)
	cmd.Env = append(os.Environ(), "LOCAL_AGENT_STATE_DIR="+h.stateDir)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	// Run with a timeout so a misconfigured command cannot hang the harness.
	if err := cmd.Start(); err != nil {
		return "", fmt.Errorf("start %s: %w", c.name, err)
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	select {
	case err := <-done:
		exitCode := 0
		if err != nil {
			if ee, ok := err.(*exec.ExitError); ok {
				exitCode = ee.ExitCode()
			} else {
				return "", fmt.Errorf("wait %s: %w", c.name, err)
			}
		}
		return formatCLIEnvelope(c.args, exitCode, stdout.String(), stderr.String(), h.redactor), nil
	case <-timeoutAfter(15 * time.Second):
		_ = cmd.Process.Kill()
		return "", fmt.Errorf("cli %s timed out", c.name)
	}
}

// formatCLIEnvelope builds the redacted text envelope for one CLI case.
func formatCLIEnvelope(args []string, exit int, stdout, stderr string, r *Redactor) string {
	var b strings.Builder
	fmt.Fprintf(&b, "$ app %s\n", strings.Join(args, " "))
	fmt.Fprintf(&b, "exit: %d\n", exit)
	b.WriteString("--- stdout ---\n")
	b.WriteString(r.String(stdout))
	if !strings.HasSuffix(stdout, "\n") && stdout != "" {
		b.WriteByte('\n')
	}
	b.WriteString("--- stderr ---\n")
	b.WriteString(r.String(stderr))
	if !strings.HasSuffix(stderr, "\n") && stderr != "" {
		b.WriteByte('\n')
	}
	return b.String()
}

// cliPairedDevice carries the device ID + secret of a device paired through
// the live server for CLI commands (devices, revoke) to target.
type cliPairedDevice struct {
	deviceID string
	secret   string
}

// pairDeviceForCLI pairs a device via the in-process server so `app devices`
// and `app revoke` have a real target. The issued secret + passcode are
// registered with the redactor. Returns the paired device's ID and secret.
//
// This calls the server handler directly (not runRESTCase) so the raw passcode
// is available for the verify step — runRESTCase redacts the passcode in the
// returned body, which would make verification impossible.
func pairDeviceForCLI(h *harness) *cliPairedDevice {
	req := httptest.NewRequest("POST", "/api/pair/initiate", strings.NewReader(`{"host":"localhost","port":7337}`))
	req.RemoteAddr = "127.0.0.1:1234"
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	h.server.Handler().ServeHTTP(rec, req)
	if rec.Code != 200 {
		return &cliPairedDevice{}
	}
	var sess struct {
		Passcode string `json:"passcode"`
		Token    string `json:"token"`
	}
	if err := jsonUnmarshal(rec.Body.Bytes(), &sess); err != nil {
		return &cliPairedDevice{}
	}
	h.redactor.RegisterSecret(sess.Passcode, "<REDACTED_PASSCODE>")
	h.redactor.RegisterSecret(sess.Token, "<REDACTED_TOKEN>")
	return pairDeviceForCLIVerify(h, sess.Passcode)
}

// pairDeviceForCLIVerify completes pairing with the given passcode and returns
// the device ID + secret, registering the secret with the redactor.
func pairDeviceForCLIVerify(h *harness, passcode string) *cliPairedDevice {
	body := fmt.Sprintf(`{"passcode":%q,"deviceName":"fixture-device"}`, passcode)
	req := httptest.NewRequest("POST", "/api/pair/verify-passcode", strings.NewReader(body))
	req.RemoteAddr = "127.0.0.1:1234"
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	h.server.Handler().ServeHTTP(rec, req)
	if rec.Code != 200 {
		return &cliPairedDevice{}
	}
	var cred struct {
		ID     string `json:"id"`
		Secret string `json:"secret"`
	}
	if err := jsonUnmarshal(rec.Body.Bytes(), &cred); err != nil {
		return &cliPairedDevice{}
	}
	h.redactor.RegisterSecret(cred.Secret, "<REDACTED_TOKEN>")
	h.redactor.RegisterSecret(cred.ID, "<REDACTED_DEVICE_ID>")
	return &cliPairedDevice{deviceID: cred.ID, secret: cred.Secret}
}

// buildApp compiles cmd/app into binPath. The binary is reused across all CLI
// cases. Build errors fail loudly.
func buildApp(repoRoot, binPath string) error {
	cmd := exec.Command("go", "build", "-o", binPath, "./cmd/app")
	cmd.Dir = repoRoot
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("build app binary: %w", err)
	}
	return nil
}

// rewriteConfigForCLI rewrites config.json in the state dir with the given port
// so the CLI's localhost:<port> URL hits the in-process httptest server. The
// seed workspace + agent are preserved.
func rewriteConfigForCLI(h *harness, port int) error {
	seedWs, err := seedWorkspacePath(harnessRepoRoot)
	if err != nil {
		return err
	}
	return writeSeedConfigWithPort(h.stateDir, seedWs, port)
}

// writePIDFile writes a daemon.pid file pointing at the harness process so the
// CLI's requireDaemonRunning check passes for API-talking commands.
func writePIDFile(h *harness) error {
	pid := os.Getpid()
	return os.WriteFile(filepath.Join(h.stateDir, "daemon.pid"), []byte(fmt.Sprintf("%d", pid)), 0o600)
}

// httptestPort extracts the TCP port from an httptest server URL.
func httptestPort(url string) int {
	// url is like http://127.0.0.1:PORT
	idx := strings.LastIndex(url, ":")
	if idx < 0 {
		return 0
	}
	var p int
	fmt.Sscanf(url[idx+1:], "%d", &p)
	return p
}

// timeoutAfter returns a channel that fires after d. It is a small helper so
// runCLICase can select on a timeout without importing time elsewhere in the
// file's main logic.
func timeoutAfter(d time.Duration) <-chan time.Time {
	return time.After(d)
}
