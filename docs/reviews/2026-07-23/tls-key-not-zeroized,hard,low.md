# TLS private key not zeroized; held in memory for process lifetime

- **Difficulty:** hard
- **Urgency:** low
- **File:** `src/app/listen.rs`
- **Lines:** 535-545

## Description

`load_tls_acceptor` reads the PEM private key via `rustls_pemfile::private_key`, passes it to `with_single_cert`, and wraps the `ServerConfig` in an `Arc` for the listener's lifetime. The key material is not zeroized and cannot be — rustls needs it for every handshake. A grep for `zeroize|Zeroize|secrecy|Secret` in `src/` finds no usage. This is largely unavoidable for a long-running TLS server, but it means a memory dump or core dump exposes the private key. Worth noting, not a fixable defect.

## Recommendation

No practical fix for a long-running server. If core dumps are a concern, disable core dumps (`prlimit`) for the daemon process. Optionally use `secrecy::SecretVec` for the in-memory key before passing to rustls (cosmetic).

## Verification

`listen.rs:538-544` loads the key and builds `ServerConfig`; `TlsAcceptor::from(Arc::new(config))` at line 545 retains it. Grep for `zeroize|secrecy` in `src/` returns only an unrelated comment in `config/tests.rs:157`.
