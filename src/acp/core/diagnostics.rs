use std::sync::{Arc, Mutex};

use futures_util::io::AsyncReadExt;

use super::STDERR_TAIL_BYTES;

pub(super) fn spawn_stderr_drain<R>(mut stderr: R, tail: Arc<Mutex<StderrTail>>)
where
    R: futures_util::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = [0_u8; 1024];
        loop {
            match stderr.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if let Ok(mut tail) = tail.lock() {
                        tail.push(&buffer[..read]);
                    }
                }
            }
        }
    });
}

/// Keywords that hint a stderr line may carry a secret. Used by
/// [`StderrTail::safe_diagnostic`] to redact credential-bearing output.
const SENSITIVE_KEYWORDS: &[&str] = &[
    "token",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "api-key",
    "secret",
    "key",
    "credential",
    "auth",
    "bearer",
    "authorization",
];

#[derive(Default)]
pub(super) struct StderrTail {
    bytes: Vec<u8>,
}

impl StderrTail {
    pub(super) fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > STDERR_TAIL_BYTES {
            let excess = self.bytes.len() - STDERR_TAIL_BYTES;
            self.bytes.drain(..excess);
        }
    }

    pub(super) fn as_string(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    /// Return a bounded startup diagnostic without obvious credential-bearing lines.
    pub(super) fn safe_diagnostic(&self) -> String {
        let diagnostic = self
            .as_string()
            .lines()
            .filter(|line| {
                let line = line.to_ascii_lowercase();
                // Security: redact any line that looks like it may carry a secret.
                // Case-insensitive substring match keeps the filter cheap and broad.
                !SENSITIVE_KEYWORDS
                    .iter()
                    .any(|keyword| line.contains(keyword))
            })
            .collect::<Vec<_>>()
            .join(" ");
        let end = diagnostic.floor_char_boundary(diagnostic.len().min(STDERR_TAIL_BYTES));
        diagnostic[..end].trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{StderrTail, STDERR_TAIL_BYTES};

    #[test]
    fn diagnostics_are_bounded_and_redact_sensitive_lines() {
        let mut tail = StderrTail::default();
        tail.push("x".repeat(STDERR_TAIL_BYTES * 2).as_bytes());
        tail.push(b"ordinary diagnostic\napi_key=super-secret\n");

        let diagnostic = tail.safe_diagnostic();
        assert!(diagnostic.len() <= STDERR_TAIL_BYTES);
        assert!(!diagnostic.contains("super-secret"));
        assert!(!diagnostic.contains("api_key"));

        let mut invalid_utf8 = StderrTail::default();
        invalid_utf8.push(&vec![0xff; STDERR_TAIL_BYTES * 2]);
        assert!(invalid_utf8.safe_diagnostic().len() <= STDERR_TAIL_BYTES);
    }
}
