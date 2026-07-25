# Pairing brute-force lockout is global, allowing trivial DoS of all pairing

- **Difficulty:** trivial
- **Urgency:** medium
- **File:** `src/pairing/mod.rs`
- **Lines:** 100-102 (Inner fields), 541-561 (check_rate_limit / record_failure), 211-237 (verify)

## Description

`failures`, `lockout_until`, and `lockout_count` live on the singleton `Inner` and are not keyed by source IP, session, or device name. Any single LAN attacker sending 5 wrong passcodes (`MAX_VERIFY_ATTEMPTS`) triggers a global lockout that escalates exponentially up to `MAX_LOCKOUT` (5 min) and only resets on a *successful* pairing (`verify`, pairing/mod.rs:233-235). An attacker who can reach `/api/pair/verify-passcode` (the per-IP HTTP token bucket in `require_pair_rate_limit` allows a 5-token burst and refills slowly, but an attacker with multiple LAN IPs or even a single IP over time) can keep the daemon permanently locked out, preventing the legitimate host user from pairing a new device. This is also a trivial denial-of-service against the recovery flow described in AGENTS.md ("additional devices authenticate from the lock screen with the mnemonic").

## Recommendation

Track `failures`/`lockout` per source IP (or per session id) instead of globally, mirroring the per-IP design already used in `require_pair_rate_limit`. Keep a global cap only as a secondary defense.

## Verification

`Inner` (pairing/mod.rs:92-104) has scalar `failures: Vec<DateTime<Utc>>`, `lockout_until`, `lockout_count` — no per-key map. `record_failure` (pairing/mod.rs:552-561) pushes to that single vec and sets the single `lockout_until`. `check_rate_limit` (pairing/mod.rs:541-550) reads the same scalar. `verify` (pairing/mod.rs:217-237) holds the single global lock for all callers.
