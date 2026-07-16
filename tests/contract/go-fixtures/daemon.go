// Package main: in-process daemon setup for the contract fixture harness.
//
// The harness constructs a fully-wired daemon (real event store, pairing
// manager, workspace manager, ACP client, sync hub, etc.) against an isolated
// temp state directory, then exercises the server's handler chain in-process
// via httptest. This avoids binding TCP ports and avoids the daemon's blocking
// Start loop, while still capturing the real handler behavior the Rust port
// must reproduce.
//
// The state directory override is communicated to the Go daemon via the
// LOCAL_AGENT_STATE_DIR environment variable (see internal/config/config.go),
// which is the single override point consulted by config.DefaultOrError and
// config.Load. Setting it before constructing the daemon ensures every
// manager writes to the temp dir.
package main

import (
	"context"
	"fmt"
	"net/http/httptest"
	"os"
	"path/filepath"
	_ "unsafe" // required by go:linkname

	"github.com/adama/local-agent/internal/daemon"
	"github.com/adama/local-agent/internal/server"
)

// harness bundles the in-process daemon, its HTTP test server, the isolated
// state directory, and a redactor loaded with the run's known secrets. It is
// the single object the per-area capturers (rest/ws/cli/dto) receive.
type harness struct {
	stateDir string
	repoRoot string
	daemon   *daemon.Daemon
	server   *server.Server
	httpSrv  *httptest.Server
	redactor *Redactor
}

// knownAgents is the unexported package-level slice of agent specs in
// internal/acp/autodetect.go. go:linkname gives the test harness access so it
// can temporarily clear the slice, preventing machine-specific autodetected
// agents (codex, cursor, devin, etc.) from polluting the golden fixtures. The
// element type is unexported (agentSpec), but a slice header is always 3 words
// regardless of element type, so []struct{} is link-compatible. Setting it to
// nil makes Autodetect() return an empty slice.
//
//go:linkname knownAgents github.com/adama/local-agent/internal/acp.knownAgents
var knownAgents []struct{}

// harnessRepoRoot is the repo root captured at harness construction so
// capturer files that need it (e.g. cli.go's config rewrite) can reach it
// without threading it through every function. It is set by newHarness.
var harnessRepoRoot string

// newHarness builds an isolated daemon + httptest server and returns a harness
// ready for fixture capture. repoRoot is used to resolve the seed workspace
// path and to build the CLI binary. The caller must call h.Close() when done.
func newHarness(repoRoot string) (*harness, error) {
	stateDir, err := os.MkdirTemp("", "local-agent-contract-")
	if err != nil {
		return nil, fmt.Errorf("create state dir: %w", err)
	}

	// Override the state dir for both the daemon construction and any CLI
	// subprocesses spawned later. config.resolvedStateDir consults this env.
	if err := os.Setenv("LOCAL_AGENT_STATE_DIR", stateDir); err != nil {
		_ = os.RemoveAll(stateDir)
		return nil, fmt.Errorf("set state dir env: %w", err)
	}

	// Write a seed config.json into the state dir BEFORE constructing the
	// daemon. daemon.New calls config.Load() internally, which reads this
	// file, so the seeded workspace and agent are loaded and registered on
	// construction. The workspace path is absolute (resolved from the
	// harness cwd, which is the repo root when run via `go run ./tests/...`).
	seedWsPath, err := seedWorkspacePath(repoRoot)
	if err != nil {
		_ = os.RemoveAll(stateDir)
		return nil, err
	}
	if err := writeSeedConfig(stateDir, seedWsPath); err != nil {
		_ = os.RemoveAll(stateDir)
		return nil, fmt.Errorf("write seed config: %w", err)
	}

	// Build a daemon config pointing at the temp dir. TLS is disabled so the
	// harness avoids self-signed cert generation and the dual HTTP+HTTPS
	// listener; the contract for REST/WS handlers is identical over plain
	// HTTP, and the WS Origin check still applies. Host is loopback so the
	// CLI's localhost URL resolves to the httptest server.
	dcfg := &daemon.Config{
		Port:                            0, // unused: handler is exercised in-process / via httptest
		Host:                            "127.0.0.1",
		DataDir:                         stateDir,
		DBPath:                          filepath.Join(stateDir, "local-agent.db"),
		TLSEnabled:                      false,
		PairingTTLSeconds:               300,
		RevocationGracePeriodSeconds:    300,
		CredentialInactivityTTLSeconds:  2592000,
	}

	// Temporarily clear the autodetect registry so the daemon's
	// autodetectAndRegisterAgents finds no machine-specific agents (codex,
	// cursor, devin, etc.). This makes the agents_list fixture portable — only
	// the fixture-agent from config.json appears. The original slice is
	// restored after daemon.New so the rest of the process is unaffected.
	origKnownAgents := knownAgents
	knownAgents = nil
	d, err := daemon.New(dcfg)
	knownAgents = origKnownAgents
	if err != nil {
		_ = os.RemoveAll(stateDir)
		return nil, fmt.Errorf("construct daemon: %w", err)
	}

	srv := d.Server()
	if srv == nil {
		d.Close()
		_ = os.RemoveAll(stateDir)
		return nil, fmt.Errorf("daemon returned nil server")
	}

	// Wrap the fully-wired handler in an httptest server so WebSocket clients
	// and CLI subprocesses can reach it over a real loopback socket. The port
	// is assigned by the kernel and discovered after Listen.
	httpSrv := httptest.NewServer(srv.Handler())

	h := &harness{
		stateDir: stateDir,
		repoRoot: repoRoot,
		daemon:   d,
		server:   srv,
		httpSrv:  httpSrv,
		redactor: NewRedactor(),
	}
	harnessRepoRoot = repoRoot
	// Scrub the temp state dir, the user's home dir, and the repo root from any
	// captured text. The repo root is registered so the seed workspace path
	// (repoRoot/tests/contract/fixtures/seed-workspace) is redacted to
	// <REDACTED_PATH>, making golden fixtures portable across machines. The
	// repo root is registered AFTER the state dir and home dir so the
	// longest-prefix-wins ordering is correct (the repo root may be under the
	// home dir on some machines, or on a separate mount like /media on others).
	h.redactor.RegisterPath(stateDir)
	if home, err := os.UserHomeDir(); err == nil {
		h.redactor.RegisterPath(home)
	}
	h.redactor.RegisterPath(repoRoot)
	// Register the httptest server's ephemeral port so it is redacted wherever
	// it appears (status output, pairing URLs, CLI output). The configured port
	// (7337) is NOT registered — it is a stable contract value that appears in
	// REST/DTO fixtures and must remain visible.
	if port := httptestPort(h.httpSrv.URL); port > 0 {
		h.redactor.RegisterSecret(fmt.Sprintf("127.0.0.1:%d", port), "127.0.0.1:<REDACTED_PORT>")
		h.redactor.RegisterSecret(fmt.Sprintf("localhost:%d", port), "localhost:<REDACTED_PORT>")
	}
	return h, nil
}

// Close tears down the httptest server and the daemon's managers. It is safe
// to call multiple times.
func (h *harness) Close() {
	if h.httpSrv != nil {
		h.httpSrv.Close()
	}
	if h.daemon != nil {
		h.daemon.Close()
	}
	// Best-effort cleanup of the temp state dir; leave it on error for
	// debugging when run with -keep-state.
	if !keepState {
		_ = os.RemoveAll(h.stateDir)
	}
}

// ctx is a background context shared by all in-process handler calls. Handler
// signatures require a context for cancellation; the harness runs serially so
// a single context is sufficient.
func (h *harness) ctx() context.Context { return context.Background() }
