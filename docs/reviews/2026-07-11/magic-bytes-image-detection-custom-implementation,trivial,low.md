# Custom Magic Bytes Image Detection

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `/media/adam/extex/projects/project-hail-larry/internal/uploads/uploads.go`
- **Lines:** 222-235

## Description

The `internal/uploads` package implements a custom `detectImage` helper function that inspects the first few bytes of a file reader against hardcoded magic-byte prefixes to identify PNG, JPEG, GIF, and WebP images.
While this works for basic formats, it is unnecessary to hand-roll magic-bytes checking when the Go standard library already provides an official, robust implementation.

## Recommendation

Replace the custom magic byte matching logic with:
1. Standard library **`http.DetectContentType`** from the `net/http` package. It reads the first 512 bytes and identifies a wide range of common image and document formats (RFC 2045).
2. Or, if strict extension matching or library-supported file-type detection is preferred, a popular library like **`github.com/h2non/filetype`**.

## Verification

Code inspection of [internal/uploads/uploads.go#L222-L235](file:///media/adam/extex/projects/project-hail-larry/internal/uploads/uploads.go#L222-L235) shows a custom function `detectImage` performing slice checks against hardcoded byte patterns (e.g. `[]byte{0x89, 'P', 'N', 'G'}`).
