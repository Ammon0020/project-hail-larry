# TestValidateCredentialSlidingWindow does not actually test sliding-window renewal

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `internal/pairing/pairing_test.go`
- **Lines:** 276-304

## Description

The test claims to verify that "activity before expiry keeps renewing the window, so a credential stays valid even after the total elapsed time since pairing exceeds the TTL." In reality, every `ValidateCredential` call is preceded by a manual `setLastSeen` to a fixed offset (`-50m`, `-40m`), so the test never depends on `ValidateCredential` actually renewing `LastSeen`. The first `setLastSeen(-90m)` at line 286 is dead code — immediately overwritten at line 290 before any validation. The test would pass identically if `ValidateCredential` did NOT renew `LastSeen` at all; it only asserts "LastSeen < TTL ⇒ valid" three times. The actual renewal behavior (the core of the feature) is only meaningfully tested by `TestValidateCredentialRenewsWithinWindow`, which checks a single renewal.

## Recommendation

Remove the manual `setLastSeen` calls between validations (or only set the initial value once), then call `ValidateCredential` multiple times with real short sleeps or a clock mock, asserting that `getLastSeen` advances after each call without manual intervention. Delete the dead `-90m` line.

## Verification

Read lines 279-304. Each of the three `ValidateCredential` calls is immediately preceded by `m.setLastSeen(cred.ID, time.Now().UTC().Add(-N*time.Minute))`, overwriting whatever `ValidateCredential` would have renewed. The `-90m` set at line 286 is overwritten at line 290 without any intervening validation.
