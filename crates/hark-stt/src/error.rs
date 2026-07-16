use thiserror::Error;

/// Errors surfaced by the cloud STT adapters. Every variant is safe to log:
/// no variant ever carries an API key, an Authorization header, or audio bytes.
#[derive(Debug, Error)]
pub enum SttError {
    /// Transport-level failure: DNS, connect refused, TLS, broken pipe.
    /// Distinguished from `Timeout` so the pipeline can pick a retry policy.
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

    /// The configured total request timeout elapsed.
    #[error("request to {provider} timed out after {configured_ms} ms")]
    Timeout {
        provider: String,
        configured_ms: u64,
    },

    /// The audio handed to an adapter (or the fixture) is not usable.
    #[error("bad audio: {0}")]
    BadAudio(String),

    /// Provider returned a non-success status or an unparseable body.
    /// `detail` is truncated so logs stay clean.
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
) -> SttError {
    match status {
        401 | 403 => SttError::Auth {
            provider: provider.to_string(),
        },
        429 => SttError::RateLimited {
            provider: provider.to_string(),
            retry_after_s,
        },
        _ => SttError::Provider {
            provider: provider.to_string(),
            detail: format!("HTTP {status}: {}", truncate_snippet(body)),
        },
    }
}

/// Map a `reqwest` transport error to the taxonomy. Timeouts are split out
/// because they are the pipeline's only retry-once candidate. `reqwest` error
/// Display strings contain the URL but never request headers or bodies, so
/// they are safe to keep as detail.
pub fn error_for_transport(provider: &str, configured_ms: u64, err: &reqwest::Error) -> SttError {
    if err.is_timeout() {
        // A timeout during connect hit the (shorter) connect bound, not the
        // total request bound the caller passes in.
        let configured_ms = if err.is_connect() {
            crate::CONNECT_TIMEOUT_MS
        } else {
            configured_ms
        };
        SttError::Timeout {
            provider: provider.to_string(),
            configured_ms,
        }
    } else {
        let kind = if err.is_connect() {
            "connect failed (no network, DNS, or provider down): "
        } else {
            ""
        };
        SttError::Http {
            provider: provider.to_string(),
            detail: format!("{kind}{err}"),
        }
    }
}
