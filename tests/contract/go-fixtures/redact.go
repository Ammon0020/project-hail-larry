// Package main: redaction helpers for the contract fixture harness.
//
// All golden fixtures must be secret-free so they can be checked in and shared.
// The redactor replaces known secret material (pairing tokens, passcodes, device
// secrets) and absolute user paths (the isolated state dir, the user's home dir,
// the fixtures source path) with stable placeholders. Known secrets are
// collected during a run — e.g. the pairing token returned by
// /api/pair/initiate — and passed to the redactor so every later occurrence
// (in REST bodies, CLI stdout, WS frames) is scrubbed.
package main

import (
	"regexp"
	"strings"
)

// Non-deterministic-value patterns. Golden fixtures must be byte-stable across
// runs so checked-in fixtures do not produce noisy diffs. Two classes of
// values are non-deterministic and are redacted to stable placeholders:
//
//   - ISO-8601 timestamps (e.g. "2026-07-16T04:49:42.091338754Z") emitted by
//     time.Time JSON marshaling. Replaced with <REDACTED_TIMESTAMP>.
//   - Long hex/base64-ish identifiers (>= 20 chars) such as pairing session
//     IDs, device IDs, upload IDs, and content hashes. Replaced with
//     <REDACTED_ID>. Short IDs (workspace IDs are 16-char hashes) are left
//     intact when they appear as standalone path segments so workspace-scoped
//     routes remain readable; the harness additionally registers the seeded
//     workspace ID with the redactor when determinism matters.
//
// The future Rust differential runner applies the SAME redactions to its own
// output before comparison, so redaction is comparison-neutral.
var (
	timestampRe = regexp.MustCompile(`\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})`)
	hexIDRe     = regexp.MustCompile(`\b[A-Fa-f0-9]{20,}\b`)
)

// Redactor scrubbs secrets and absolute paths out of fixture text. It is safe
// for concurrent use only from a single goroutine; the harness runs serially.
type Redactor struct {
	// secrets maps a raw secret string to the placeholder that replaces it.
	secrets map[string]string
	// paths is a list of absolute path prefixes to replace with <REDACTED_PATH>.
	// Longer prefixes are matched first so a nested dir is replaced before its
	// parent would match a shorter substring.
	paths []string
}

// NewRedactor returns an empty Redactor.
func NewRedactor() *Redactor {
	return &Redactor{secrets: make(map[string]string)}
}

// RegisterSecret records a raw secret value and the placeholder to substitute
// for it. The same secret may be registered multiple times; the last
// placeholder wins. Empty secrets are ignored (they would otherwise erase
// every empty string).
func (r *Redactor) RegisterSecret(raw, placeholder string) {
	if raw == "" {
		return
	}
	r.secrets[raw] = placeholder
}

// RegisterPath records an absolute path prefix to replace with <REDACTED_PATH>.
// Paths are matched longest-first so nested directories are scrubbed before
// their parents.
func (r *Redactor) RegisterPath(prefix string) {
	if prefix == "" {
		return
	}
	r.paths = append(r.paths, prefix)
	// Sort longest-first so the most specific prefix wins during replacement.
	// A simple insertion sort: paths is tiny (a handful of entries).
	for i := len(r.paths) - 1; i > 0; i-- {
		if len(r.paths[i]) > len(r.paths[i-1]) {
			r.paths[i], r.paths[i-1] = r.paths[i-1], r.paths[i]
		}
	}
}

// String returns a redacted copy of s. Replacement order:
//  1. Registered secrets (may contain path-like substrings; scrubbed first so
//     a secret that embeds a registered path keeps its own placeholder).
//  2. Registered absolute path prefixes (longest first).
//  3. Non-deterministic timestamps and long hex IDs (regex backstop) so the
//     golden files are byte-stable across runs.
func (r *Redactor) String(s string) string {
	for raw, placeholder := range r.secrets {
		s = strings.ReplaceAll(s, raw, placeholder)
	}
	for _, prefix := range r.paths {
		s = strings.ReplaceAll(s, prefix, "<REDACTED_PATH>")
	}
	s = timestampRe.ReplaceAllString(s, "<REDACTED_TIMESTAMP>")
	s = hexIDRe.ReplaceAllString(s, "<REDACTED_ID>")
	return s
}

// Bytes is a convenience wrapper around String for byte slices.
func (r *Redactor) Bytes(b []byte) []byte {
	return []byte(r.String(string(b)))
}

// hexTokenPattern matches long hex/base64-ish token strings embedded in JSON
// values (e.g. pairing tokens, device secrets) that were not explicitly
// registered. It is a defense-in-depth backstop in case a secret slips through
// without being registered. Tokens shorter than 16 chars are left alone so
// short IDs are preserved for readability and stable comparison.
var hexTokenPattern = regexp.MustCompile(`"(token|secret|secretHash)"\s*:\s*"([A-Za-z0-9_\-]{16,})"`)

// ScrubUnregisteredTokens applies the regex backstop to any remaining token-like
// JSON fields the harness did not explicitly register. It is idempotent: a
// placeholder already in place is left as-is.
func ScrubUnregisteredTokens(s string) string {
	return hexTokenPattern.ReplaceAllStringFunc(s, func(match string) string {
		// Only replace the value group, keep the field name.
		sub := hexTokenPattern.FindStringSubmatch(match)
		if len(sub) < 3 {
			return match
		}
		return `"` + sub[1] + `":"<REDACTED_TOKEN>"`
	})
}
