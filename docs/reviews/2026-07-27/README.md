# Comprehensive Code Review - 2026-07-27

## Scope Reviewed
- Batch 1 of 1
- Subagents: 4 dispatched
- Surface area covered: `src/app/`, `src/api/`, `src/acp/core/`, `src/permissions/`, `src/events/`, `src/sync/`, `src/config/`, and `src/files/`.

## Findings Summary
Total Findings: 10
- High Urgency: 1
- Medium Urgency: 5
- Low Urgency: 4

### High Urgency
| Finding | Difficulty | Description |
|---|---|---|
| [unix-process-kill-zero-group-signal](unix-process-kill-zero-group-signal,easy,high.md) | Easy | `libc::kill(0, sig)` sends signals to the caller's process group on Unix. |

### Medium Urgency
| Finding | Difficulty | Description |
|---|---|---|
| [allow-always-session-scoping](allow-always-session-scoping,med,medium.md) | Med | `AllowAlways` / `RejectAlways` incorrectly scoped to a single session. |
| [http-to-https-redirect-ipv6-host-split](http-to-https-redirect-ipv6-host-split,easy,medium.md) | Easy | HTTP-to-HTTPS redirect logic incorrectly splits IPv6 hosts without ports. |
| [tls-cert-zero-byte-reuse-bypass](tls-cert-zero-byte-reuse-bypass,easy,medium.md) | Easy | Zero-byte TLS cert files mistakenly bypass generation and cause crash. |
| [sync-write-pump-ping-stall](sync-write-pump-ping-stall,medium,medium.md) | Med | `write_pump` ping loop stalls outbound text events for up to 10s. |
| [sync-lagged-resync-missing-note-delivered](sync-lagged-resync-missing-note-delivered,easy,medium.md) | Easy | Replay misses `note_delivered`, causing duplicate broadcasts. |

### Low Urgency
| Finding | Difficulty | Description |
|---|---|---|
| [audit-log-vector-drain-performance](audit-log-vector-drain-performance,easy,low.md) | Easy | Suboptimal O(N) vector drain used in audit logging. |
| [permission-manager-cleanup-stale-duplication](permission-manager-cleanup-stale-duplication,easy,low.md) | Easy | Code duplication between `get_pending` and `cleanup_stale`. |
| [proc-net-pid-lookup-unnecessary-string-alloc](proc-net-pid-lookup-unnecessary-string-alloc,easy,low.md) | Easy | Unnecessary string allocation during PID lookup. |
| [config-default-relative-path-fallback](config-default-relative-path-fallback,easy,low.md) | Easy | Config falls back to relative paths when home dir is missing. |

## Notes
- `src/acp/core/` is architecturally clean and robust with no findings.
- The review was paused after Batch 1 to focus on resolving these 7 findings.
