# src/app/

## Responsibility

Daemon core server initialization, network listener setup, TLS security, rate limiting, and process lifecycle management.

## Module Map

```text
src/app/
├── mod.rs          entry/exports
├── daemon.rs       state, workers, shutdown
├── listen.rs       sockets and connection accept
├── logging.rs      tracing/file-console sinks
├── port.rs         port selection/conflicts
├── process.rs      subprocess/signal management
├── rate_limit.rs   request rate limiting
├── tls.rs          TLS listener/configuration
└── tls_cert.rs     certificate generation/loading
```

## Rules & Patterns

- **Network Security**: Bind daemon only to designated interface addresses (default local network / localhost); enforce TLS for all non-loopback connections.
- **Graceful Termination**: Ensure signal handlers trigger clean shutdown of background workers, active ACP sessions, and SQLite database connections.
- **Zero Panic Listener**: Network errors during connection acceptance or TLS handshake must log warnings and continue serving without panicking the daemon process.
