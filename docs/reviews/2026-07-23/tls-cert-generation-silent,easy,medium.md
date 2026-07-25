# Self-signed certificate (re)generation is not logged loudly

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `src/app/tls_cert.rs`
- **Lines:** 34-76

## Description

`ensure_self_signed` generates a brand-new self-signed CA-less certificate and writes it to disk with no logging at all. `listen.rs:94` calls it and discards whether a new cert was created vs. reused. The module doc (lines 1-6) correctly notes that silently replacing a cert "would break paired devices and weakens trust-on-first-use behavior," and the function avoids overwriting existing files (line 39), but the *first* generation (or any future scenario where the files are deleted) happens with zero `info!`/`warn!`. For a TOFU model this is the moment the operator must consciously trust a fingerprint; silence is a security smell. If an attacker with write access to `tls_cert_dir` deletes the cert/key, the daemon will quietly mint a new one on next start with no operator-visible signal.

## Recommendation

Have `ensure_self_signed` return whether it generated vs. reused, and have `listen::bind` log an `info!`/`warn!` with the certificate fingerprint (SHA-256 of the DER) when a new cert is created. Consider logging the fingerprint on every start for operator verification.

## Verification

`tls_cert.rs:34-76` contains no `tracing`/`log` calls. `listen.rs:89-95` calls `ensure_self_signed` and `load_tls_acceptor` with no logging of the generation event.
