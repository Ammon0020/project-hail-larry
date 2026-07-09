# newID crypto/rand fallback produces an ID that fails isValidID, making the upload unretrievable

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `internal/uploads/uploads.go`
- **Lines:** 138-148

## Description

On `rand.Read` failure, `newID` returns the literal `"0000000000000000"` (16 characters) as a 'unique-ish' fallback. But `isValidID` (lines 152-162) requires exactly 32 hex characters. Consequently, if the fallback ever fires, `Store` succeeds and writes `<16-char-id>.<ext>` to disk, but `Get(sessionID, uploadID)` rejects the ID at line 109 (`isValidID` fails) before doing the prefix search — so the upload is permanently orphaned and the ID returned to the HTTP client (api.go:601) can never be used to retrieve the file. The comment claims the fallback lets Store 'still produce a unique-ish name'; in reality it produces a name that the package's own validator rejects. `crypto/rand.Read` failing is near-impossible on modern platforms, so severity is low, but the fallback is logically wrong and would cause silent data loss if it ever triggered.

## Recommendation

Make the fallback 32 hex characters long (e.g. `return strings.Repeat("0", 32)`) so it passes `isValidID`, or — better — panic on rand failure since a working CSPRNG is a hard runtime requirement and a silent fixed ID risks collisions. If a non-panicking fallback is desired, generate it from a fallback entropy source and ensure it is 32 hex chars.

## Verification

Line 145 returns `"0000000000000000"`; counting the characters gives 16. `isValidID` at lines 153-160 requires `len(id) != 32` to fail. The on-disk filename at line 89 is `uploadID + ext`, and `Get`'s prefix search (line 119, `uploadID + "."`) would technically match, but `Get` returns early at line 110 because `isValidID(uploadID)` is false for a 16-char ID.
