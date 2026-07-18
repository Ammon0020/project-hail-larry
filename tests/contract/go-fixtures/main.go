// Package main is the Go-side compatibility fixture generator for story
// S-CONTRACT (docs/plans/rust-port/active-S-CONTRACT-compatibility-med.md).
//
// It captures golden fixtures FROM the current Go daemon so the future Rust
// port can prove external equivalence via a differential runner. The harness
// constructs a fully-wired daemon against an isolated temp state directory
// (LOCAL_AGENT_STATE_DIR), exercises every REST route, the WebSocket hub, the
// CLI, and shared DTO serialization, and writes redacted fixtures under
// tests/contract/golden/.
//
// Usage (from the repo root):
//
//	go run ./tests/contract/go-fixtures/
//	go run ./tests/contract/go-fixtures/ -keep-state   # leave temp dir for debugging
//
// The generator does NOT write the Rust-side differential runner; that comes
// later when the Rust implementation exists. See tests/contract/README.md for
// the regeneration workflow and the comparison rules the future runner will
// apply.
package main

import (
	"flag"
	"fmt"
	"log"
	"os"
	"path/filepath"
)

func main() {
	flag.BoolVar(&keepState, "keep-state", false, "leave the isolated state dir in place after the run for debugging")
	flag.Parse()

	goldenDir, repoRoot, err := resolvePaths()
	if err != nil {
		fail(err)
	}
	if err := generate(goldenDir, repoRoot); err != nil {
		fail(err)
	}
	log.Printf("S-CONTRACT: done -> %s", goldenDir)
}

// generate runs the full capture pipeline (REST, WS, DTO, CLI) into goldenDir.
// It is extracted from main so it can also be invoked from a test
// (TestGenerateFixtures) in environments where `go run` is sandboxed.
func generate(goldenDir, repoRoot string) error {
	log.Printf("S-CONTRACT: golden dir = %s", goldenDir)
	log.Printf("S-CONTRACT: repo root  = %s", repoRoot)

	// Reset golden/ so removed/renamed routes do not leave stale fixtures.
	if err := resetGolden(goldenDir); err != nil {
		return err
	}

	h, err := newHarness(repoRoot)
	if err != nil {
		return err
	}
	defer h.Close()
	log.Printf("S-CONTRACT: state dir  = %s", h.stateDir)
	log.Printf("S-CONTRACT: http srv   = %s", h.httpSrv.URL)

	if err := captureREST(h, goldenDir); err != nil {
		return err
	}
	log.Printf("S-CONTRACT: REST fixtures captured")

	if err := captureWS(h, goldenDir); err != nil {
		return err
	}
	log.Printf("S-CONTRACT: WebSocket fixtures captured")

	if err := captureDTO(h, goldenDir); err != nil {
		return err
	}
	log.Printf("S-CONTRACT: DTO fixtures captured")

	// Reset the pairing rate limiters before CLI capture so `app pair` has a
	// fresh budget. The REST capture phase consumed the limiter via
	// /api/pair/initiate; without a reset the CLI pair command hits 429 and
	// cascades into empty devices/revoke fixtures.
	resetPairRateLimiters()

	if err := captureCLI(h, goldenDir, repoRoot); err != nil {
		return err
	}
	log.Printf("S-CONTRACT: CLI fixtures captured")
	return nil
}

// resolvePaths returns the golden directory (tests/contract/golden) and the
// repo root. It works whether the cwd is the repo root (the `go run` path) or
// the harness package dir (the `go test` path, where cwd is
// tests/contract/go-fixtures/). It walks up from cwd looking for go.mod.
func resolvePaths() (goldenDir, repoRoot string, err error) {
	cwd, err := os.Getwd()
	if err != nil {
		return "", "", fmt.Errorf("get cwd: %w", err)
	}
	// Walk up at most a few levels looking for a go.mod. The harness lives at
	// <repo>/tests/contract/go-fixtures, so the repo root is at most 3
	// parents above the package-dir cwd, and is the cwd itself when run via
	// `go run` from the repo root.
	dir := cwd
	for i := 0; i < 6; i++ {
		if _, statErr := os.Stat(filepath.Join(dir, "go.mod")); statErr == nil {
			repoRoot = dir
			break
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	if repoRoot == "" {
		return "", "", fmt.Errorf("could not locate repo root (go.mod) from %s", cwd)
	}
	goldenDir = filepath.Join(repoRoot, "tests", "contract", "golden")
	return goldenDir, repoRoot, nil
}

// resetGolden removes and recreates the golden subdirectories so stale
// fixtures from removed/renamed routes do not linger. The golden/ dir itself
// is preserved (it may contain a .gitkeep).
func resetGolden(goldenDir string) error {
	subs := []string{"rest", "cli", "dto", "ws"}
	for _, sub := range subs {
		dir := filepath.Join(goldenDir, sub)
		if err := os.RemoveAll(dir); err != nil {
			return fmt.Errorf("reset %s: %w", dir, err)
		}
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return fmt.Errorf("mkdir %s: %w", dir, err)
		}
	}
	return nil
}

// fail logs the error and exits with a non-zero status so CI catches failures
// loudly (AGENTS.md: fail loudly).
func fail(err error) {
	log.Printf("S-CONTRACT: FAILED: %v", err)
	os.Exit(1)
}
