# Story S-ACP-PROVIDERS: ACP Provider Management

> **Phase:** 3 | **Depends on:** S-ACP-CORE | **Go source:** `internal/acp/providers.go`

## Goal

Port the unstable ACP provider list/set/disable capability behind a narrow
capability check and stable API error behavior.

## Acceptance Criteria

- [ ] Provider list, set, and disable use only verified current SDK APIs
- [ ] Unsupported-agent behavior maps to the existing API response contract
- [ ] Provider changes are scoped to the correct session and do not reset history
- [ ] Provider contract and integration tests pass
