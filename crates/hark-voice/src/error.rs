use thiserror::Error;

/// Errors surfaced by the cleanup adapter. Every variant is safe to log: no
/// variant ever carries an API key, an Authorization header, a prompt, or
/// transcript text. Mirrors `SttError`'s taxonomy deliberately; a shared error
/// crate between hark-stt and hark-voice is conscious non-work (two small
/// parallel enums beat a premature abstraction).
#[derive(Debug, Error)]
pub enum CleanupError {
    /// Transport-level failure: DNS, connect refused, TLS, broken pipe.
    /// Distinguished from `Timeout` so logs name the failure class; the
    /// pipeline treats every cleanup error the same (fail-open, no retry).
    #[error("http error ({provider}): {detail}")]
    Http { provider: String, detail: String },

    /// 401/403 from the provider. The message never echoes the key.
    #[error("authentication rejected by {provider}: check your API key")]
    Auth { provider: String },

    /// 429 from the provider. `retry_after_s` comes from the Retry-After
    /// header when present.
    #[error("rate limited by {provider} (retry-after: {retry_after_s:?} s)")]
    RateLimited {
        provider: String,
        retry_after_s: Option<u64>,
    },

    /// The per-request cleanup timeout elapsed.
    #[error("request to {provider} timed out after {configured_ms} ms")]
    Timeout {
        provider: String,
        configured_ms: u64,
    },

    /// Provider returned a non-success status, an unparseable body, or empty
    /// content. `detail` is truncated so logs stay clean.
    #[error("provider error ({provider}): {detail}")]
    Provider { provider: String, detail: String },
}

/// Cap provider body snippets so an error never drags a huge (or binary)
/// response body into logs.
pub(crate) const BODY_SNIPPET_MAX: usize = 300;

pub(crate) fn truncate_snippet(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= BODY_SNIPPET_MAX {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(BODY_SNIPPET_MAX).collect();
    format!("{cut}…")
}

/// Map a non-success HTTP status to the error taxonomy. Pure so the mapping is
/// unit-testable without a network.
pub fn error_for_status(
    provider: &str,
    status: u16,
    retry_after_s: Option<u64>,
    body: &str,
) -> CleanupError {
    match status {
        401 | 403 => CleanupError::Auth {
            provider: provider.to_string(),
        },
        429 => CleanupError::RateLimited {
            provider: provider.to_string(),
            retry_after_s,
        },
        _ => CleanupError::Provider {
            provider: provider.to_string(),
            detail: format!("HTTP {status}: {}", truncate_snippet(body)),
        },
    }
}

/// Map a `reqwest` transport error to the taxonomy. `reqwest` error Display
/// strings contain the URL but never request headers or bodies, so they are
/// safe to keep as detail. The CP0 spike verifies that buffered JSON bodies
/// keep `is_timeout()`/`is_connect()` classifiable (the multipart masking bug,
/// LL-G HIGH, must not reproduce here).
pub fn error_for_transport(
    provider: &str,
    configured_ms: u64,
    err: &reqwest::Error,
) -> CleanupError {
    if err.is_timeout() {
        // A timeout during connect hit the (shorter) connect bound of the
        // shared client, not the per-request bound the caller passes in.
        let configured_ms = if err.is_connect() {
            crate::CONNECT_TIMEOUT_MS
        } else {
            configured_ms
        };
        CleanupError::Timeout {
            provider: provider.to_string(),
            configured_ms,
        }
    } else {
        let kind = if err.is_connect() {
            "connect failed (no network, DNS, or provider down): "
        } else {
            ""
        };
        CleanupError::Http {
            provider: provider.to_string(),
            detail: format!("{kind}{err}"),
        }
    }
}
