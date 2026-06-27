// Package search implements workspace-wide file-content search.
//
// Search uses ripgrep (rg) when available on PATH for speed, and falls back to
// a Go-native walker (filepath.WalkDir + bufio.Scanner + regexp) when rg is
// missing. Both strategies honor the same options (case-insensitivity, a file
// name glob filter, context lines, and a max-results cap) and skip the same
// set of noise directories and hidden files to match the file-tree behavior in
// internal/workspace.
package search

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
)

// SearchOptions controls a search run.
type SearchOptions struct {
	// Pattern is the regular expression to search for (required).
	Pattern string
	// IgnoreCase makes the pattern case-insensitive when true.
	IgnoreCase bool
	// MaxResults caps the number of match results returned. Defaults to 200
	// when <= 0 so a giant workspace cannot exhaust memory.
	MaxResults int
	// FilePattern is an optional glob restricting which file paths are
	// searched (e.g. "*.go"). Empty means all files.
	FilePattern string
	// ContextLines is the number of context lines to include before and after
	// each match. Currently only used to configure rg; the Go fallback reports
	// only the matched line (offsets are still accurate).
	ContextLines int
}

// SearchResult is a single match within a file.
type SearchResult struct {
	// Path is the file path relative to the workspace root.
	Path string `json:"path"`
	// LineNumber is the 1-based line number of the match.
	LineNumber int `json:"lineNumber"`
	// LineContent is the full text of the matched line.
	LineContent string `json:"lineContent"`
	// MatchStart is the 0-based byte offset within LineContent where the
	// match begins.
	MatchStart int `json:"matchStart"`
	// MatchEnd is the 0-based byte offset within LineContent where the match
	// ends (exclusive).
	MatchEnd int `json:"matchEnd"`
}

// defaultMaxResults is used when SearchOptions.MaxResults is <= 0.
const defaultMaxResults = 200

// ignoreDirs are directory names that are always skipped during the walk. They
// match the file-tree behavior in internal/workspace plus common build/dep
// caches that would otherwise produce noisy matches.
var ignoreDirs = map[string]bool{
	".git":         true,
	"node_modules": true,
	"vendor":       true,
	"dist":         true,
	"build":        true,
	".next":        true,
	"target":       true,
	".cache":       true,
	"coverage":     true,
	"out":          true,
}

// rgAvailable caches whether ripgrep is on PATH. It is resolved once via
// sync.Once so repeated searches do not re-shell out to `which`/`where`.
var (
	rgOnce      sync.Once
	rgAvailable bool
)

// rgOnPath reports whether the ripgrep binary is available on PATH.
func rgOnPath() bool {
	rgOnce.Do(func() {
		_, err := exec.LookPath("rg")
		rgAvailable = err == nil
	})
	return rgAvailable
}

// Search runs a content search rooted at root and returns up to opts.MaxResults
// matches. All returned paths are relative to root; absolute paths are never
// returned. The ctx is honored for cancellation/timeout in both the rg and
// Go-fallback strategies.
func Search(ctx context.Context, root string, opts SearchOptions) ([]SearchResult, error) {
	if strings.TrimSpace(opts.Pattern) == "" {
		return nil, errors.New("search pattern is required")
	}
	if opts.MaxResults <= 0 {
		opts.MaxResults = defaultMaxResults
	}

	// Compile the pattern up front so both strategies share the same validation.
	// A bad regex is surfaced as a user error (the caller maps it to HTTP 400).
	// Go's regexp.Compile does not take flags, so case-insensitivity is applied
	// via the (?i) inline flag prefix.
	pattern := opts.Pattern
	if opts.IgnoreCase {
		pattern = "(?i)" + pattern
	}
	re, err := regexp.Compile(pattern)
	if err != nil {
		return nil, fmt.Errorf("invalid pattern: %w", err)
	}

	if rgOnPath() {
		return searchWithRg(ctx, root, opts, re)
	}
	return searchWithGo(ctx, root, opts, re)
}

// searchWithRg shells out to ripgrep and parses its --json output. rg is fast
// and respects .gitignore by default; we additionally pass --hidden and our
// own -g negations so the skipped-directory set matches the Go fallback.
func searchWithRg(ctx context.Context, root string, opts SearchOptions, re *regexp.Regexp) ([]SearchResult, error) {
	args := []string{
		"--json",
		"--no-config",
		"--hidden",
		"-n",
		"--max-count", strconv.Itoa(opts.MaxResults),
	}
	if opts.IgnoreCase {
		args = append(args, "--ignore-case")
	}
	if opts.ContextLines > 0 {
		args = append(args, "-C", strconv.Itoa(opts.ContextLines))
	}
	// Negate the same directories skipped by the Go fallback so the two
	// strategies produce consistent results.
	for dir := range ignoreDirs {
		args = append(args, "-g", "!"+dir)
	}
	// Also skip hidden files/dirs (leading dot) to match the file tree.
	args = append(args, "-g", "!.*")
	if opts.FilePattern != "" {
		args = append(args, "-g", opts.FilePattern)
	}
	args = append(args, opts.Pattern, root)

	cmd := exec.CommandContext(ctx, "rg", args...)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		// rg exits with code 1 when there are no matches — that is not an error.
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) && exitErr.ExitCode() == 1 {
			return nil, nil
		}
		// A context cancellation is expected (debounce/typing); surface it.
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, fmt.Errorf("rg failed: %w: %s", err, strings.TrimSpace(stderr.String()))
	}

	return parseRgJSON(stdout.Bytes(), root, re, opts.MaxResults)
}

// parseRgJSON parses ripgrep's --json output, collecting match lines into
// SearchResult values. Only "match" record types are emitted; context/summary
// records are ignored so each result is a real hit.
func parseRgJSON(data []byte, root string, re *regexp.Regexp, max int) ([]SearchResult, error) {
	var results []SearchResult
	sc := bufio.NewScanner(bytes.NewReader(data))
	// rg --json lines can be long (full file lines); raise the per-line cap.
	sc.Buffer(make([]byte, 0, 64*1024), 1024*1024)
	for sc.Scan() {
		line := sc.Bytes()
		if len(line) == 0 {
			continue
		}
		// We avoid pulling in encoding/json to keep the package stdlib-only.
		// Each rg --json record is a single object on one line; we extract the
		// "type" field to decide whether to parse it as a match.
		recType := jsonStringValue(line, "type")
		if recType != "match" {
			continue
		}

		pathStr := jsonStringValue(line, "path", "text")
		if pathStr == "" {
			continue
		}
		relPath, err := filepath.Rel(root, pathStr)
		if err != nil {
			continue
		}
		// Never surface absolute paths.
		relPath = filepath.ToSlash(relPath)

		lineNum := jsonIntValue(line, "line_number")
		lineText := jsonStringValue(line, "line", "text")
		if lineText == "" {
			// Some rg versions nest the line under "data"."text".
			lineText = jsonNestedStringValue(line, "data", "text")
		}

		// Compute match offsets from the first submatch on the line text. rg
		// provides "submatches" but recomputing via the compiled regex keeps
		// the offsets consistent with the Go fallback.
		start, end := 0, 0
		if loc := re.FindStringIndex(lineText); loc != nil {
			start, end = loc[0], loc[1]
		}

		results = append(results, SearchResult{
			Path:        relPath,
			LineNumber:  lineNum,
			LineContent: lineText,
			MatchStart:  start,
			MatchEnd:    end,
		})
		if len(results) >= max {
			break
		}
	}
	if err := sc.Err(); err != nil {
		return results, fmt.Errorf("parse rg output: %w", err)
	}
	return results, nil
}

// searchWithGo is the stdlib fallback: it walks the tree with filepath.WalkDir,
// skips noise directories and binary files, and scans each file line-by-line
// with the compiled regex.
func searchWithGo(ctx context.Context, root string, opts SearchOptions, re *regexp.Regexp) ([]SearchResult, error) {
	var fileFilter *regexp.Regexp
	if opts.FilePattern != "" {
		// Convert a glob like "*.go" into an anchored regex.
		globRe, err := globToRegex(opts.FilePattern)
		if err != nil {
			return nil, fmt.Errorf("invalid file pattern: %w", err)
		}
		fileFilter = globRe
	}

	var results []SearchResult
	walkErr := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			// Skip unreadable entries rather than aborting the whole search.
			return nil
		}
		if ctx.Err() != nil {
			return ctx.Err()
		}

		base := d.Name()
		if d.IsDir() {
			// Skip hidden directories and known noise dirs.
			if strings.HasPrefix(base, ".") || ignoreDirs[base] {
				return filepath.SkipDir
			}
			return nil
		}

		// Skip hidden files.
		if strings.HasPrefix(base, ".") {
			return nil
		}
		// Apply the file-name glob filter (matched against the base name).
		if fileFilter != nil && !fileFilter.MatchString(base) {
			return nil
		}
		// Skip symlinks to avoid cycles, matching the file-tree behavior.
		if d.Type()&os.ModeSymlink != 0 {
			return nil
		}

		relPath, rerr := filepath.Rel(root, path)
		if rerr != nil {
			return nil
		}
		relPath = filepath.ToSlash(relPath)

		fileResults, ferr := searchFile(ctx, path, relPath, re, opts.MaxResults-len(results))
		if ferr != nil {
			// Skip files we can't read (permission, vanished, etc.).
			return nil
		}
		results = append(results, fileResults...)
		if len(results) >= opts.MaxResults {
			return errStopWalk
		}
		return nil
	})
	if walkErr != nil && !errors.Is(walkErr, errStopWalk) {
		return results, walkErr
	}
	return results, nil
}

// errStopWalk is a sentinel returned from WalkDir to stop traversal once the
// result cap is reached. It is filtered out by the caller.
var errStopWalk = errors.New("search: max results reached")

// searchFile scans a single file for matches and returns one SearchResult per
// matching line, up to remaining slots. Binary files (detected via null bytes
// in the first 512 bytes) are skipped.
func searchFile(ctx context.Context, absPath, relPath string, re *regexp.Regexp, remaining int) ([]SearchResult, error) {
	if remaining <= 0 {
		return nil, nil
	}
	f, err := os.Open(absPath) //nolint:gosec // absPath is within the workspace root.
	if err != nil {
		return nil, err
	}
	defer f.Close()

	// Binary detection: sample the first 512 bytes for null bytes.
	sample := make([]byte, 512)
	n, _ := io.ReadFull(f, sample)
	if bytes.IndexByte(sample[:n], 0) >= 0 {
		return nil, nil
	}
	// Rewind so the scanner sees the whole file from the start.
	if _, err := f.Seek(0, io.SeekStart); err != nil {
		return nil, err
	}

	var results []SearchResult
	sc := bufio.NewScanner(f)
	// Allow long lines without truncation.
	sc.Buffer(make([]byte, 0, 64*1024), 1024*1024)
	lineNum := 0
	for sc.Scan() {
		lineNum++
		if ctx.Err() != nil {
			return results, ctx.Err()
		}
		line := sc.Text()
		loc := re.FindStringIndex(line)
		if loc == nil {
			continue
		}
		results = append(results, SearchResult{
			Path:        relPath,
			LineNumber:  lineNum,
			LineContent: line,
			MatchStart:  loc[0],
			MatchEnd:    loc[1],
		})
		if len(results) >= remaining {
			return results, nil
		}
	}
	if err := sc.Err(); err != nil {
		return results, err
	}
	return results, nil
}

// globToRegex converts a simple glob (with * and ?) into an anchored regex.
// It does not support character classes or braces to keep the implementation
// small; rg handles full globs natively.
func globToRegex(glob string) (*regexp.Regexp, error) {
	var sb strings.Builder
	sb.WriteString("^")
	for _, r := range glob {
		switch r {
		case '*':
			sb.WriteString(".*")
		case '?':
			sb.WriteByte('.')
		case '.', '+', '(', ')', '|', '[', ']', '{', '}', '^', '$', '\\':
			sb.WriteByte('\\')
			sb.WriteRune(r)
		default:
			sb.WriteRune(r)
		}
	}
	sb.WriteString("$")
	return regexp.Compile(sb.String())
}

// jsonStringValue extracts a string value for a top-level key from a single
// line of JSON. Nested keys (e.g. "path"."text") may be passed as additional
// args to descend one level. This avoids a full encoding/json dependency for
// the rg parser; it is intentionally minimal and only handles the shapes rg
// emits.
func jsonStringValue(data []byte, keys ...string) string {
	for i, key := range keys {
		needle := []byte(`"` + key + `"`)
		idx := bytes.Index(data, needle)
		if idx < 0 {
			return ""
		}
		rest := data[idx+len(needle):]
		// Find the colon after the key.
		colon := bytes.IndexByte(rest, ':')
		if colon < 0 {
			return ""
		}
		rest = rest[colon+1:]
		// Skip whitespace.
		rest = bytes.TrimLeft(rest, " \t")
		if len(rest) == 0 || rest[0] != '"' {
			// Not a string value; for nested keys, descend into the object.
			if i < len(keys)-1 {
				// Find the opening brace of the nested object and continue.
				objIdx := bytes.IndexByte(rest, '{')
				if objIdx < 0 {
					return ""
				}
				data = rest[objIdx:]
				continue
			}
			return ""
		}
		// Parse the string literal starting at rest[0].
		rest = rest[1:]
		var sb strings.Builder
		for j := 0; j < len(rest); j++ {
			c := rest[j]
			if c == '\\' && j+1 < len(rest) {
				j++
				switch rest[j] {
				case '"':
					sb.WriteByte('"')
				case '\\':
					sb.WriteByte('\\')
				case '/':
					sb.WriteByte('/')
				case 'n':
					sb.WriteByte('\n')
				case 't':
					sb.WriteByte('\t')
				case 'r':
					sb.WriteByte('\r')
				case 'b':
					sb.WriteByte('\b')
				case 'f':
					sb.WriteByte('\f')
				case 'u':
					if j+4 < len(rest) {
						if r, err := strconv.ParseUint(string(rest[j+1:j+5]), 16, 32); err == nil {
							sb.WriteRune(rune(r))
							j += 4
						}
					}
				default:
					sb.WriteByte(rest[j])
				}
				continue
			}
			if c == '"' {
				break
			}
			sb.WriteByte(c)
		}
		return sb.String()
	}
	return ""
}

// jsonNestedStringValue is a thin wrapper for the common two-level nesting
// ("data"."text") used by some rg versions for match line content.
func jsonNestedStringValue(data []byte, keys ...string) string {
	return jsonStringValue(data, keys...)
}

// jsonIntValue extracts an integer value for a top-level key from a single
// line of JSON. Returns 0 when the key is absent or the value is not a number.
func jsonIntValue(data []byte, key string) int {
	needle := []byte(`"` + key + `"`)
	idx := bytes.Index(data, needle)
	if idx < 0 {
		return 0
	}
	rest := data[idx+len(needle):]
	colon := bytes.IndexByte(rest, ':')
	if colon < 0 {
		return 0
	}
	rest = rest[colon+1:]
	rest = bytes.TrimLeft(rest, " \t")
	end := 0
	for end < len(rest) && (rest[end] == '-' || (rest[end] >= '0' && rest[end] <= '9')) {
		end++
	}
	if end == 0 {
		return 0
	}
	v, err := strconv.Atoi(string(rest[:end]))
	if err != nil {
		return 0
	}
	return v
}
