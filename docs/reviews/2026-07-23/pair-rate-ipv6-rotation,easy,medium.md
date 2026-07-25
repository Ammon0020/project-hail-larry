# Pairing rate limit defeated by IPv6 address rotation (no /64 aggregation)

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `src/api/mod.rs`
- **Lines:** 1334-1338, 1356-1361

## Description

The rate-limit key is the full peer IP string (`connect.0.ip().to_string()`). `verify-token` is the brute-force target for QR/pairing tokens, and it is rate-limited (good), but an attacker with a /64 IPv6 allocation gets a fresh 5-request burst bucket for every distinct address. This effectively nullifies the 5/minute cap on the brute-forceable `verify-token` endpoint. Combined with the unbounded-bucket finding, the same rotation also drives unbounded memory growth.

## Recommendation

Normalize IPv6 peers to their /64 prefix (zero the low 64 bits) before keying the bucket, so a single attacker subnet shares one bucket. Consider a global fallback cap in addition to per-IP.

## Verification

`require_pair_rate_limit` (line 1337) uses `connect.0.ip().to_string()` verbatim as the peer key; `allow_pair_request` (line 1357) keys `buckets.entry(peer.to_string())` with no prefix normalization.
