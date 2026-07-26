//! Daemon port liveness and PID discovery.
//!
//! `start`/`stop` historically keyed only off the `daemon.pid` file. When a
//! daemon process kept holding the configured HTTP port but its PID file was
//! missing (orphaned process, different binary, or interrupted cleanup), the
//! CLI was stuck: `stop` reported "not running" and `start` failed at `bind`
//! with a confusing `Address already in use`. The helpers here let the CLI
//! detect a port-holding orphan and recover it.
//!
//! All helpers are best-effort and never panic: a probe failure is treated as
//! "not listening" so the normal bind path can produce the real OS error.

use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

use anyhow::Result;

/// Maximum time to wait for a TCP connect when probing a port.
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// Return whether something is listening on `port` at `host`.
///
/// `host` follows the same convention as `Config::host`: empty or a wildcard
/// (`0.0.0.0` / `::`) probes the loopback address, since a wildcard-bound
/// daemon accepts on every interface including 127.0.0.1. Any connect error
/// (refused, timeout, name resolution) is reported as "not listening" so the
/// caller can fall through to the real `bind` error path.
#[must_use]
pub fn is_port_listening(host: &str, port: u16) -> bool {
    let probe_host = if host.is_empty() || host == "0.0.0.0" || host == "::" {
        "127.0.0.1"
    } else {
        host
    };
    // Parse as an IP (not SocketAddr, which requires a port) so a bare
    // address like "127.0.0.1" is accepted. Non-IP host names are not
    // expected for the daemon bind address; treat them as not listening
    // rather than spinning up a resolver.
    let Ok(ip) = probe_host.parse::<IpAddr>() else {
        return false;
    };
    let target = SocketAddr::new(ip, port);
    TcpStream::connect_timeout(&target, PROBE_TIMEOUT).is_ok()
}

/// Find a process listening on `port`, when supported by the platform.
///
/// Returns `Ok(None)` on platforms without a cheap kernel introspection path
/// (non-Linux). On Linux, parses `/proc/net/tcp` and `/proc/net/tcp6` for
/// LISTEN sockets on the port and resolves the owning PID via `/proc/<pid>/fd`.
/// If multiple processes match, the first one encountered is returned.
///
/// # Errors
///
/// Returns an error if `/proc` cannot be read or parsed on Linux.
pub fn find_pid_listening_on(port: u16) -> Result<Option<u32>> {
    #[cfg(target_os = "linux")]
    {
        find_pid_listening_on_linux(port)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = port;
        Ok(None)
    }
}

#[cfg(target_os = "linux")]
fn find_pid_listening_on_linux(port: u16) -> Result<Option<u32>> {
    use anyhow::Context;
    use std::fs;

    // /proc/net/tcp row layout (whitespace-split, 0-indexed):
    //   0 sl  1 local_address  2 rem_address  3 st  4 tx:rx  5 tr:tm
    //   6 retrnsmt  7 uid  8 timeout  9 inode ...
    // local_address is "HEXIP:HEXPORT"; st == "0A" means LISTEN.
    let port_hex = format!("{port:04X}");
    let listen_state = "0A";
    let mut inodes: Vec<u64> = Vec::new();
    for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 10 {
                continue;
            }
            if fields[3] != listen_state {
                continue;
            }
            // fields[1] looks like "0100007F:1CA9"; the port is after the colon.
            let Some((_, port_part)) = fields[1].split_once(':') else {
                continue;
            };
            if !port_part.eq_ignore_ascii_case(&port_hex) {
                continue;
            }
            if let Ok(inode) = fields[9].parse::<u64>() {
                if inode != 0 {
                    inodes.push(inode);
                }
            }
        }
    }
    if inodes.is_empty() {
        return Ok(None);
    }

    // Resolve each matching socket inode to a PID via /proc/<pid>/fd symlinks.
    let proc_entries = fs::read_dir("/proc").context("read /proc")?;
    for entry in proc_entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let name = name.to_string();
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        let Ok(fd_entries) = fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fd_entries.flatten() {
            let Ok(link) = fs::read_link(fd.path()) else {
                continue;
            };
            let Some(s) = link.to_str() else { continue };
            let Some(rest) = s.strip_prefix("socket:[") else {
                continue;
            };
            let Some(num_str) = rest.strip_suffix(']') else {
                continue;
            };
            let Ok(num) = num_str.parse::<u64>() else {
                continue;
            };
            if inodes.contains(&num) {
                return Ok(Some(pid));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A bound listener must be reported as listening on its port, and a
    /// different (free) port must not. Uses loopback so the probe matches the
    /// daemon's wildcard-bind behavior.
    #[test]
    fn is_port_listening_detects_bound_and_free_ports() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = listener.local_addr().expect("local addr").port();

        // Wildcard host must probe loopback, matching daemon bind semantics.
        assert!(is_port_listening("0.0.0.0", port));
        assert!(is_port_listening("127.0.0.1", port));
        assert!(is_port_listening("", port));

        // A high ephemeral port is overwhelmingly likely to be free; retry a
        // few times to avoid a rare false positive from a concurrent binder.
        let mut saw_free = false;
        for candidate in 50000u16..50050u16 {
            if candidate == port {
                continue;
            }
            if !is_port_listening("127.0.0.1", candidate) {
                saw_free = true;
                break;
            }
        }
        assert!(saw_free, "expected at least one free port in probe range");

        drop(listener);
    }

    /// A non-IP host must not panic or hang; the probe reports not listening.
    #[test]
    fn is_port_listening_handles_non_ip_host() {
        assert!(!is_port_listening("not-a-host", 7337));
    }

    /// On Linux, `find_pid_listening_on` resolves the PID owning a bound
    /// listener. The test process itself owns the listener, so the result must
    /// match `std::process::id()`.
    #[cfg(target_os = "linux")]
    #[test]
    fn find_pid_listening_on_locates_bound_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = listener.local_addr().expect("local addr").port();
        let me = std::process::id();
        let found = find_pid_listening_on(port).expect("proc parse");
        assert_eq!(found, Some(me));
        drop(listener);
    }

    /// A free port must yield no PID.
    #[cfg(target_os = "linux")]
    #[test]
    fn find_pid_listening_on_returns_none_for_free_port() {
        // Bind and immediately drop to grab a recently-free port number.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);
        // The kernel may briefly retain the port in TIME_WAIT, but LISTEN
        // resolution should still be absent. Retry to tolerate races.
        let mut resolved_none = false;
        for _ in 0..10 {
            match find_pid_listening_on(port).expect("proc parse") {
                None => {
                    resolved_none = true;
                    break;
                }
                Some(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        assert!(
            resolved_none,
            "expected no LISTEN owner for freed port {port}"
        );
    }
}
