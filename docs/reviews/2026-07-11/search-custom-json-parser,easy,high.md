# Custom JSON Parser in Workspace Search

- **Difficulty:** easy
- **Urgency:** high
- **File:** `/media/adam/extex/projects/project-hail-larry/internal/search/search.go`
- **Lines:** 402-484

## Description

The `internal/search` package processes the JSON output of `ripgrep` lines using a custom-rolled, state-less scanner (`jsonStringValue`, `jsonNestedStringValue`, `jsonIntValue` helpers). The codebase contains a comment stating:
`"We avoid pulling in encoding/json to keep the package stdlib-only."`

This is a misconception, as `encoding/json` is a core Go standard library package. Hand-rolling custom byte-scanning parser code is brittle and prone to bugs when handling escaped characters (e.g. `\"`, `\\`), control characters, varying whitespace, or nested object formatting.

## Recommendation

Replace the custom JSON parsing helper logic with either:
1. Standard library **`encoding/json`** using a simple struct for target fields.
2. If reflection performance is a concern, use a fast, reflection-free JSON selection library like **`github.com/tidwall/gjson`** or **`github.com/buger/jsonparser`**.

Both solutions are robust, handle escaped strings, and eliminate the custom byte-scanning helper code.

## Verification

Code inspection of [internal/search/search.go](file:///media/adam/extex/projects/project-hail-larry/internal/search/search.go#L402-L484) shows `jsonStringValue` and `jsonIntValue` searching for index sequences of brackets, quotes, and commas to manually extract raw text.
