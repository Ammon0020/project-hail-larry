# Stored device secret hash uses plain SHA-256 (no salt, no slow KDF)

- **Difficulty:** medium
- **Urgency:** low
- **File:** `src/pairing/mod.rs`
- **Lines:** 639-641 (hash_secret), 430 (issue_credential), 250 (validate_credential)

## Description

`hash_secret` is `hex::encode(Sha256::digest(secret.as_bytes()))` — unsalted, single-pass SHA-256. The device secret itself is 256-bit random (`random_hex(32)`, pairing/mod.rs:425), so offline preimage attack against a leaked `devices.json` is computationally infeasible and this is **not directly exploitable** today. However, the pattern is fragile: there is no salt, so identical secrets would produce identical hashes (not currently possible given 256-bit entropy), and the fast hash gives no defense-in-depth if a future change lowers secret entropy (e.g., a mnemonic-only credential path). The module header comment (lines 3-5) advertises "SHA-256 hashes" as the protection, which understates the real protection (the 256-bit secret entropy, not the hash).

## Recommendation

Use a slow, salted KDF (argon2 / bcrypt / scrypt) for `secret_hash` so that the storage layer is robust independent of secret entropy. Keep the 256-bit random secret regardless. This also future-proofs any lower-entropy credential added later.

## Verification

`hash_secret` (pairing/mod.rs:639-641) is a single `Sha256::digest` with no salt, used by `issue_credential` (line 430) and `validate_credential` (line 242/250). No `argon2`/`bcrypt`/`scrypt` dependency is present in the imports (pairing/mod.rs:15-23).
