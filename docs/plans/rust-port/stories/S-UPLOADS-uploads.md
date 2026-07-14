# Story S-UPLOADS: File Upload Store

> **Phase:** 2 | **Depends on:** — | **Go source:** `internal/uploads/` (244 lines)

## Summary

Port the upload store: receives file uploads (images for prompts), stores
on disk with opaque IDs, serves them back to the frontend.

## Go Source

`internal/uploads/` — `Manager`, stores files in `~/.local-agent/uploads/`,
generates UUID IDs, validates MIME types, serves by ID.

## Rust Implementation

- UUID: `uuid` crate (v4)
- MIME detection: `infer` crate (magic-byte detection, replaces
  `http.DetectContentType`)
- Store path: `~/.local-agent/uploads/`
- Port tests

## Acceptance Criteria

- [ ] Uploads stored with opaque UUID IDs
- [ ] MIME type validated via magic bytes
- [ ] Files served by ID with correct Content-Type
- [ ] `cargo test uploads` passes
