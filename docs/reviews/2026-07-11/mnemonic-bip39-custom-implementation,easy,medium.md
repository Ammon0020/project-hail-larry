# Custom BIP-39 Mnemonic Word List and Generation

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `/media/adam/extex/projects/project-hail-larry/internal/pairing/words.go`
- **Lines:** 1-214

## Description

The `internal/pairing` package includes a fully copy-pasted copy of the BIP-39 English word list (2048 entries) in `words.go`, spanning 20KB of static source code. Additionally, `pairing.go` implements its own custom random-word selection logic using `crypto/rand` to generate passcodes.
While functional, carrying a generated copy of a standard word list increases repository noise and ignores existing battle-tested cryptographic libraries.

## Recommendation

Replace the custom word list and random generation logic with the standard Go BIP-39 library:
- **`github.com/tyler-smith/go-bip39`**

This library provides audited BIP-39 mnemonic generation, handles entropy generation safely, and includes the BIP-39 English word list out of the box, allowing the deletion of the custom `words.go` file.

## Verification

Code inspection of [internal/pairing/words.go](file:///media/adam/extex/projects/project-hail-larry/internal/pairing/words.go) shows the copy-pasted static string array, and [internal/pairing/pairing.go#L624-L635](file:///media/adam/extex/projects/project-hail-larry/internal/pairing/pairing.go#L624-L635) shows the custom selection wrapper using `rand.Int`.
