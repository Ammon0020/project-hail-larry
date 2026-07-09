# Migration backfill from PairedAt expires old devices on upgrade

- **Difficulty:** medium
- **Urgency:** high
- **File:** `internal/pairing/pairing.go`
- **Lines:** 572-581

## Description

`loadDevices` migrates devices persisted before the `LastSeen` field existed by backfilling `r.LastSeen = r.PairedAt`. The comment claims this gives legacy devices "a fair sliding window rather than being treated as instantly expired," but for any device paired more than `inactivityTTL` ago (default 30 days), the very next `ValidateCredential` computes `now.Sub(stored.LastSeen) > m.inactivityTTL` as true and rejects the credential (lines 412-414). On upgrade, every user whose device was paired >30 days ago is silently logged out and forced to re-pair. This is the opposite of "a fair sliding window" for long-tenured devices. The migration only behaves fairly for recently-paired devices.

## Recommendation

Backfill migrated devices with `LastSeen = time.Now().UTC()` (giving them a fresh full window from upgrade time), or mark them with a sentinel and skip the expiry check on their first validation so they get a clean renewal. Setting to `PairedAt` is only safe when `PairedAt` is within the TTL.

## Verification

Read `loadDevices` (lines 554-582) and `ValidateCredential` (lines 407-414). The expiry check is `now.Sub(stored.LastSeen) > m.inactivityTTL`; with `LastSeen = PairedAt` and `PairedAt` > 30 days old, this is unconditionally true on the first post-upgrade validation. `TestLoadDevicesMigratesLastSeen` (lines 321-351) only tests with `pairedAt = now - 24h`, which is inside the 30-day default TTL, so it never exercises the failure case.
