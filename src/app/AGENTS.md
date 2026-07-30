# src/app/

## Responsibility

Daemon core server initialization, network listener setup, TLS security, rate limiting, and process lifecycle management.

## Module Map

- **`mod.rs`** — Daemon subsystem entry and module exports.
- **`daemon.rs`** — Main daemon state initialization, background worker coordination, and graceful shutdown signal handling.
- **`listen.rs`** — Network socket binding, dual IPv4/IPv6 listener configuration, and connection accepting.
- **`logging.rs`** — Tracing and log output configuration for file and console sinks.
- **`port.rs`** — Dynamic port selection and conflict detection for daemon startup.
- **`process.rs`** — Subprocess execution management and signal propagation.
- **`rate_limit.rs`** — Request rate-limiting middleware to guard against local network abuse.
- **`tls.rs`** / **`tls_cert.rs`** — Self-signed TLS certificate generation, loading, and rustls configuration.

## Rules & Patterns

- **Network Security**: Bind daemon only to designated interface addresses (default local network / localhost); enforce TLS for all non-loopback connections.
- **Graceful Termination**: Ensure signal handlers trigger clean shutdown of background workers, active ACP sessions, and SQLite database connections.
- **Zero Panic Listener**: Network errors during connection acceptance or TLS handshake must log warnings and continue serving without panicking the daemon process.
