// Package main: test entry point that runs the full fixture pipeline.
//
// In sandboxed environments where `go run ./tests/contract/go-fixtures/` is
// blocked (the harness execs a built CLI binary), `go test` is still allowed,
// so this test invokes the same generate() function main() calls. It is a
// smoke test that the harness runs end-to-end and produces a non-empty
// golden/ tree; it is NOT a differential test (that lives in the future Rust
// runner).
package main

import (
	"os"
	"path/filepath"
	"testing"
)

// TestGenerateFixtures runs the full capture pipeline and asserts that each
// golden subdirectory is non-empty. The golden files themselves are the
// contract surface and are checked in; this test regenerates them in place.
func TestGenerateFixtures(t *testing.T) {
	goldenDir, repoRoot, err := resolvePaths()
	if err != nil {
		t.Fatal(err)
	}
	if err := generate(goldenDir, repoRoot); err != nil {
		t.Fatalf("generate: %v", err)
	}

	for _, sub := range []string{"rest", "cli", "dto", "ws"} {
		dir := filepath.Join(goldenDir, sub)
		entries, err := os.ReadDir(dir)
		if err != nil {
			t.Fatalf("read golden/%s: %v", sub, err)
		}
		if len(entries) == 0 {
			t.Fatalf("golden/%s is empty after generate", sub)
		}
		t.Logf("golden/%s: %d files", sub, len(entries))
	}
}
