# Duplicate session-creation boilerplate across 4 new tests

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `tests/acp_core_lifecycle.rs`
- **Lines:** 175-184, 200-209, 224-233, 237-242

## Description

Each of the 4 new tests repeats the same 4-line session-creation block:

```rust
let session = tokio::time::timeout(
    ACTOR_TIMEOUT,
    client.create_session("mockagent", "mock-model", &workspace_id),
)
.await
.expect("session creation timed out")
.expect("create mockagent session");
```

This pattern appears in all 4 new tests (lines 175-184, 200-209, 224-233,
237-242) plus the 2 pre-existing tests (lines 33-42, 113-122). A helper
function would eliminate the repetition:

```rust
async fn create_mock_session(
    client: &Client,
    workspace_id: &str,
) -> local_agent::interfaces::Session {
    tokio::time::timeout(
        ACTOR_TIMEOUT,
        client.create_session("mockagent", "mock-model", workspace_id),
    )
    .await
    .expect("session creation timed out")
    .expect("create mockagent session")
}
```

This is consistent with the existing style (the 2 pre-existing tests also
inline this pattern), so it is not a regression. But with 6 total call
sites now, the duplication is more noticeable.

## Recommendation

Extract a `create_mock_session` helper and replace the 6 inline blocks
(4 new + 2 existing). This is a minor refactor — keep it optional unless
the file grows further.

## Verification

Read `tests/acp_core_lifecycle.rs` lines 175-184, 200-209, 224-233, and
237-242 — all four blocks are byte-identical (same agent name, model,
timeout, and expect messages). The 2 pre-existing tests at lines 33-42
and 113-122 use the same pattern.
