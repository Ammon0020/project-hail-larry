package search

import (
	"context"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"
)

// makeFixture builds a small temp workspace tree for the search tests:
//
//	root/
//	  a.go      -> "package foo\nfunc Hello() {}\n"
//	  b.txt     -> "TODO: fix this\nnothing here\n"
//	  sub/
//	    c.go    -> "var TODO = true\n"
//	  .hidden   -> "TODO secret\n"   (must be skipped)
//	  node_modules/
//	    d.go    -> "TODO noise\n"    (must be skipped)
//
// It returns the root path.
func makeFixture(t *testing.T) string {
	t.Helper()
	root := t.TempDir()

	mustWrite := func(rel, content string) {
		full := filepath.Join(root, filepath.FromSlash(rel))
		if err := os.MkdirAll(filepath.Dir(full), 0755); err != nil {
			t.Fatalf("mkdir: %v", err)
		}
		if err := os.WriteFile(full, []byte(content), 0644); err != nil {
			t.Fatalf("write %s: %v", rel, err)
		}
	}

	mustWrite("a.go", "package foo\nfunc Hello() {}\n")
	mustWrite("b.txt", "TODO: fix this\nnothing here\n")
	mustWrite("sub/c.go", "var TODO = true\n")
	mustWrite(".hidden", "TODO secret\n")
	mustWrite("node_modules/d.go", "TODO noise\n")

	return root
}

// findResult returns the SearchResult matching a given path, or nil.
func findResult(results []SearchResult, path string) *SearchResult {
	for i := range results {
		if results[i].Path == path {
			return &results[i]
		}
	}
	return nil
}

// TestSearch_GoFallback exercises the stdlib walker directly by forcing it via
// a pattern that rg would also find, so the test passes whether or not rg is
// installed. It asserts path, line number, and match offsets.
func TestSearch_GoFallback(t *testing.T) {
	if rgOnPath() {
		t.Skip("rg is installed; this test targets the Go fallback — run with rg absent to exercise it")
	}
	root := makeFixture(t)

	results, err := Search(context.Background(), root, SearchOptions{
		Pattern:    "TODO",
		IgnoreCase: false,
		MaxResults: 50,
	})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}

	// Expected matches: b.txt line 1, sub/c.go line 1. The hidden file and
	// node_modules dir must be skipped.
	if len(results) != 2 {
		t.Fatalf("expected 2 results, got %d: %+v", len(results), results)
	}

	sort.Slice(results, func(i, j int) bool { return results[i].Path < results[j].Path })

	r0 := results[0]
	if r0.Path != "b.txt" {
		t.Errorf("results[0].Path = %q, want b.txt", r0.Path)
	}
	if r0.LineNumber != 1 {
		t.Errorf("results[0].LineNumber = %d, want 1", r0.LineNumber)
	}
	if r0.LineContent != "TODO: fix this" {
		t.Errorf("results[0].LineContent = %q, want %q", r0.LineContent, "TODO: fix this")
	}
	if r0.MatchStart != 0 || r0.MatchEnd != 4 {
		t.Errorf("results[0] offsets = [%d,%d), want [0,4)", r0.MatchStart, r0.MatchEnd)
	}

	r1 := results[1]
	if r1.Path != "sub/c.go" {
		t.Errorf("results[1].Path = %q, want sub/c.go", r1.Path)
	}
	if r1.LineNumber != 1 {
		t.Errorf("results[1].LineNumber = %d, want 1", r1.LineNumber)
	}
	if r1.MatchStart != 4 || r1.MatchEnd != 8 {
		t.Errorf("results[1] offsets = [%d,%d), want [4,8)", r1.MatchStart, r1.MatchEnd)
	}
}

// TestSearch_IgnoreCase verifies case-insensitive matching finds "todo" in
// "TODO: fix this".
func TestSearch_IgnoreCase(t *testing.T) {
	if rgOnPath() {
		t.Skip("rg installed; Go fallback test skipped")
	}
	root := makeFixture(t)

	results, err := Search(context.Background(), root, SearchOptions{
		Pattern:    "todo",
		IgnoreCase: true,
		MaxResults: 50,
	})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}

	if findResult(results, "b.txt") == nil {
		t.Errorf("expected a match in b.txt with IgnoreCase, got: %+v", results)
	}
}

// TestSearch_FilePattern verifies the file-name glob filter restricts which
// files are searched.
func TestSearch_FilePattern(t *testing.T) {
	if rgOnPath() {
		t.Skip("rg installed; Go fallback test skipped")
	}
	root := makeFixture(t)

	results, err := Search(context.Background(), root, SearchOptions{
		Pattern:     "TODO",
		FilePattern: "*.go",
		MaxResults:  50,
	})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}

	// Only sub/c.go should match (a.go has no TODO; b.txt is filtered out by
	// the glob; node_modules is skipped).
	if len(results) != 1 {
		t.Fatalf("expected 1 result for *.go, got %d: %+v", len(results), results)
	}
	if results[0].Path != "sub/c.go" {
		t.Errorf("result path = %q, want sub/c.go", results[0].Path)
	}
}

// TestSearch_InvalidRegex verifies a bad pattern surfaces as an error the
// caller can map to a 400.
func TestSearch_InvalidRegex(t *testing.T) {
	root := makeFixture(t)
	_, err := Search(context.Background(), root, SearchOptions{
		Pattern: "[unclosed",
	})
	if err == nil {
		t.Fatal("expected error for invalid regex, got nil")
	}
}

// TestSearch_EmptyPattern verifies an empty pattern is rejected.
func TestSearch_EmptyPattern(t *testing.T) {
	root := makeFixture(t)
	_, err := Search(context.Background(), root, SearchOptions{Pattern: ""})
	if err == nil {
		t.Fatal("expected error for empty pattern, got nil")
	}
}

// TestParseRgJSON_LineContentNotPath guards against a regression where the
// rg JSON parser put the file path into LineContent instead of the matched
// line text. rg's --json output nests line content under "data"."lines"."text"
// and the file path under "data"."path"."text"; a previous version looked up
// the wrong key ("line" instead of "lines") and fell back to "data"."text",
// which matched "data"."path"."text" (the file path) first.
func TestParseRgJSON_LineContentNotPath(t *testing.T) {
	root := t.TempDir()
	fullPath := filepath.Join(root, "b.txt")
	// JSON-escape backslashes in the path (Windows paths use \).
	escapedPath := strings.ReplaceAll(fullPath, `\`, `\\`)
	re := regexp.MustCompile(`TODO`)
	input := `{"type":"match","data":{"path":{"text":"` + escapedPath +
		`"},"lines":{"text":"TODO: fix this"},"line_number":1,` +
		`"absolute_offset":0,"submatches":[{"match":{"text":"TODO"},"start":0,"end":4}]}}` + "\n"

	results, err := parseRgJSON([]byte(input), root, re, 50)
	if err != nil {
		t.Fatalf("parseRgJSON: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d: %+v", len(results), results)
	}
	r := results[0]
	if r.LineContent != "TODO: fix this" {
		t.Errorf("LineContent = %q, want %q", r.LineContent, "TODO: fix this")
	}
	if r.Path != "b.txt" {
		t.Errorf("Path = %q, want b.txt", r.Path)
	}
	if r.LineNumber != 1 {
		t.Errorf("LineNumber = %d, want 1", r.LineNumber)
	}
	if r.MatchStart != 0 || r.MatchEnd != 4 {
		t.Errorf("offsets = [%d,%d), want [0,4)", r.MatchStart, r.MatchEnd)
	}
}

// TestSearch_RelativePaths verifies all returned paths are relative to root
// (never absolute).
func TestSearch_RelativePaths(t *testing.T) {
	if rgOnPath() {
		t.Skip("rg installed; Go fallback test skipped")
	}
	root := makeFixture(t)
	results, err := Search(context.Background(), root, SearchOptions{
		Pattern: "TODO",
	})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}
	for _, r := range results {
		if filepath.IsAbs(r.Path) {
			t.Errorf("result path %q is absolute; expected relative to root", r.Path)
		}
	}
}
