# Cleartext HTTP listener remains active on the LAN even when TLS is enabled

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `src/app/listen.rs`
- **Lines:** 75-103, 124-146

## Description

`bind` always binds a cleartext TCP listener on `config.host:config.port` (line 77) and only *adds* an HTTPS listener when `tls_enabled` (line 96). `serve` runs both listeners on the **same router** (lines 110, 114, 129, 154-156). When `host=0.0.0.0`, the cleartext listener is reachable from the LAN and serves the full authenticated API, so an attacker on the network can simply use `http://` instead of `https://` to send Bearer credentials and preview cookies in cleartext. The only guard is a `warn!` at line 83 that fires *only when TLS is disabled* — when TLS is enabled but host is `0.0.0.0`, there is no warning that cleartext HTTP is still exposed on the LAN. The `TlsConnection` extension (which gates the `Secure` flag on the preview cookie, line 1227) is only inserted on the HTTPS path (line 155), so preview cookies issued over the HTTP listener are never marked `Secure`.

## Recommendation

When `tls_enabled` and `is_wildcard_host(host)`, either (a) bind the cleartext listener to loopback only, or (b) refuse to start, or (c) emit a warning. At minimum, extend the existing `warn!` (line 83) to also fire when TLS is enabled but the cleartext listener is wildcard-bound.

## Verification

`serve_http` (line 129) serves the full `router` over cleartext with `into_make_service_with_connect_info` but no `TlsConnection` layer; `serve_https` (line 155) adds `.layer(Extension(TlsConnection))`. `require_auth` line 1227 reads `TlsConnection` to decide the `Secure` flag, so HTTP-issued preview cookies never get `Secure`.
