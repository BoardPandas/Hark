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
    ///
    /// Same reasoning as `hark_stt::SttError::Auth`: 403 is usually quota or
    /// project access, not a bad key, and the two used to be indistinguishable
    /// in the UI -- including from an STT failure, since the strings matched
    /// word for word.
    #[error("cleanup authentication rejected by {provider} (HTTP {status}){}", detail_suffix(.status, .detail))]
    Auth {
        provider: String,
        status: u16,
        detail: String,
    },

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
    /// content. `detail` contains only status/category and structural diagnostics.
    #[error("provider error ({provider}): {detail}")]
    Provider { provider: String, detail: String },
}

/// Serde's Display can quote an invalid string value. Only its structural
/// category and numeric location are safe to include in diagnostics.
pub(crate) fn json_error_detail(context: &str, error: &serde_json::Error) -> String {
    format!(
        "{context} (JSON {:?} at line {}, column {})",
        error.classify(),
        error.line(),
        error.column()
    )
}

/// The tail of an auth error message: the provider's own reason code when it
/// gave one, and the key hint only for a 401, where it is the likely cause.
fn detail_suffix(status: &u16, detail: &str) -> String {
    if !detail.is_empty() {
        return format!(": {detail}");
    }
    if *status == 401 {
        return ": check your API key".to_string();
    }
    String::new()
}

/// The provider's own machine-readable reason for a 401/403, and nothing else.
///
/// Deliberately NOT the response body. A provider's auth error frequently
/// quotes the key back ("Incorrect API key provided: sk-..."), so echoing the
/// body would put the secret in a log line via the one error guaranteed to be
/// shown to the user. OpenAI-shaped errors carry `error.code` and `error.type`
/// -- only known slugs like `invalid_api_key`, `insufficient_quota`, and
/// `model_not_found` are accepted. Unknown values can also carry secrets.
fn auth_reason(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return String::new();
    };
    let err = v.get("error").unwrap_or(&v);
    for field in ["code", "type"] {
        if let Some(slug) = err.get(field).and_then(|s| s.as_str()) {
            let slug = slug.trim();
            // A provider can put arbitrary content in these fields too.
            // Preserve only known diagnostic labels, never an unknown value.
            if matches!(
                slug,
                "invalid_api_key"
                    | "insufficient_quota"
                    | "model_not_found"
                    | "authentication_error"
                    | "permission_denied"
                    | "permission_error"
                    | "invalid_request_error"
                    | "account_deactivated"
                    | "organization_deactivated"
                    | "access_denied"
                    | "UNAUTHENTICATED"
                    | "PERMISSION_DENIED"
            ) {
                return slug.to_string();
            }
        }
    }
    String::new()
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
            status,
            detail: auth_reason(body),
        },
        429 => CleanupError::RateLimited {
            provider: provider.to_string(),
            retry_after_s,
        },
        _ => CleanupError::Provider {
            provider: provider.to_string(),
            detail: format!("HTTP {status}"),
        },
    }
}

/// Map a `reqwest` transport error to safe categories; its Display can expose
/// credentials or user content in the URL. The CP0 spike verifies that buffered JSON bodies
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
            "connect failed (no network, DNS, or provider down)"
        } else if err.is_builder() {
            "request configuration is invalid"
        } else if err.is_body() {
            "request or response body transfer failed"
        } else if err.is_decode() {
            "response decoding failed"
        } else if err.is_redirect() {
            "redirect failed"
        } else {
            "request transport failed"
        };
        CleanupError::Http {
            provider: provider.to_string(),
            detail: kind.to_string(),
        }
    }
}
