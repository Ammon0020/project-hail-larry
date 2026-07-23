# Compaction finding — workspace-preview / S-PREVIEW-LAN-AUTH

> **Status:** finding (not applied — non-obvious tradeoff). **Date:** 2026-07-22.

## Location

`src/api/mod.rs` preview-token tests (lines ~2262–2404):

- `preview_session_cookie_authenticates_relative_asset`
- `tls_preview_entry_sets_secure_cookie`
- `preview_token_wrong_workspace_is_rejected`

## Opportunity

All three tests repeat the same ~12-line preamble:

1. `pending_actions_state(0, false)` → `(dir, state, cred)`
2. `preview_fixture_workspace(&state).await` → `(_site, ws)`
3. `oneshot_peer` POST to `/api/workspaces/{ws.id}/preview-session` with
   `Authorization: Bearer {cred}`
4. `json_body(session).await["token"].as_str().expect("preview token").to_owned()`

A helper such as `async fn mint_preview_token(state, cred, ws_id) -> String`
would collapse each preamble to one call and remove the duplicated
`oneshot_peer`/`json_body`/bearer wiring.

## Why not applied now

- The TLS test (`tls_preview_entry_sets_secure_cookie`) inserts
  `TlsConnection` into the entry request **after** minting the token, so the
  helper would only cover the mint step — the saving is smaller than it
  appears and the call sites would still differ in shape.
- `preview_token_wrong_workspace_is_rejected` mints against `first.id` but
  hits the entry URL with `second.id`; a helper that takes `ws_id` would
  read fine, but the test's intent (cross-workspace rejection) is currently
  spelled out inline and is easy to follow.
- Test readability favors explicit request construction for auth-sensitive
  paths; a helper adds an indirection that a future reviewer has to follow
  to confirm the bearer header and peer address are correct.

## Recommendation

Leave as-is unless a fourth preview-token test is added, at which point the
duplicated mint preamble crosses the "three-or-more" threshold and a helper
clearly pays off. Record here so the next reviewer can decide with context.
