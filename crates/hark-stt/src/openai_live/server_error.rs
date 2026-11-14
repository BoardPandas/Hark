//! A server's account of a failure, reduced to what is safe to keep, and
//! its place in the error taxonomy the HTTP adapters use.

use super::{fail, PROVIDER};
use crate::error::SttError;
use serde_json::Value;

/// A server error, reduced to the fields that are safe to keep.
///
/// The free-text `message` is never read: OpenAI quotes a masked key in it.
/// `code`, `type` and `param` are kept only when they are slug-shaped
/// (lowercase ASCII, digits, `_`, `.`, `[`, `]`), which every documented value
/// is and no key or transcript text would be.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteError {
    pub code: Option<String>,
    pub kind: Option<String>,
    pub param: Option<String>,
}

impl RemoteError {
    pub(super) fn from_value(error: Option<&Value>) -> Self {
        let field = |name: &str| {
            error
                .and_then(|e| e.get(name))
                .and_then(Value::as_str)
                .and_then(safe_slug)
        };
        RemoteError {
            code: field("code"),
            kind: field("type"),
            param: field("param"),
        }
    }

    fn describe(&self) -> String {
        let mut out = self
            .code
            .clone()
            .or_else(|| self.kind.clone())
            .unwrap_or_else(|| "no error code".to_string());
        if self.code.is_some() {
            if let Some(kind) = &self.kind {
                out.push_str(&format!(" ({kind})"));
            }
        }
        if let Some(param) = &self.param {
            out.push_str(&format!(" at {param}"));
        }
        out
    }
}

/// `value`, if it is a protocol identifier rather than free text.
pub(super) fn safe_slug(value: &str) -> Option<String> {
    let value = value.trim();
    let ok = (1..=96).contains(&value.len())
        && value.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '[' | ']')
        });
    ok.then(|| value.to_string())
}

/// Map a server error onto the taxonomy the HTTP adapters use.
///
/// | code / type | error |
/// |---|---|
/// | `invalid_api_key`, type `authentication_error` | `Auth`, 401 |
/// | `insufficient_quota`, `account_deactivated`, `organization_deactivated`, `permission_denied`, `access_denied`, type `permission_error` | `Auth`, 403 |
/// | `rate_limit_exceeded`, type `rate_limit_error` | `RateLimited` |
/// | anything else | `Provider`, with the slug-shaped code, type and param |
///
/// A socket refusal has no HTTP status, so `Auth` carries the status OpenAI's
/// REST API uses for the same refusal (close code 3000 is the WebSocket
/// registry's "Unauthorized", 3003 its "Forbidden"). 403 keeps the meaning
/// `SttError::Auth` documents — quota, model access or org policy, not a bad
/// key — so a quota problem is not reported as one that waiting fixes.
pub fn remote_error(context: &str, error: &RemoteError) -> SttError {
    let code = error.code.as_deref();
    let kind = error.kind.as_deref();
    let auth = |status: u16, slug: &str| SttError::Auth {
        provider: PROVIDER.to_string(),
        status,
        detail: slug.to_string(),
    };
    match (code, kind) {
        (Some(slug @ "invalid_api_key"), _) => auth(401, slug),
        (
            Some(
                slug @ ("insufficient_quota"
                | "account_deactivated"
                | "organization_deactivated"
                | "permission_denied"
                | "access_denied"),
            ),
            _,
        ) => auth(403, slug),
        (Some("rate_limit_exceeded"), _) | (None, Some("rate_limit_error")) => {
            SttError::RateLimited {
                provider: PROVIDER.to_string(),
                retry_after_s: None,
            }
        }
        (None, Some(slug @ "authentication_error")) => auth(401, slug),
        (None, Some(slug @ "permission_error")) => auth(403, slug),
        _ => fail(format!("{context}: {}", error.describe())),
    }
}

/// The error for a close frame that arrived before the transcript.
///
/// 3000 and 3003 are how OpenAI closes over a refused key or forbidden
/// access; their reason (`invalid_request_error.invalid_api_key`) is kept only
/// as its slug-shaped last segment. Any other reason is free text and dropped.
pub fn close_error(code: u16, reason: &str, phase: &str) -> SttError {
    let slug = || {
        reason
            .rsplit('.')
            .next()
            .and_then(safe_slug)
            .unwrap_or_default()
    };
    match code {
        3000 => SttError::Auth {
            provider: PROVIDER.to_string(),
            status: 401,
            detail: slug(),
        },
        3003 => SttError::Auth {
            provider: PROVIDER.to_string(),
            status: 403,
            detail: slug(),
        },
        _ => fail(format!(
            "the session closed {phase} (WebSocket close code {code})"
        )),
    }
}
