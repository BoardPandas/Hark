//! The retry-decision predicate. Latency is the product: at most ONE retry
//! per dictation, and only for failures where a retry can plausibly succeed
//! immediately (timeouts and connect-class transport errors). Never retry
//! 4xx: a bad key or a rate limit will not get better in 200 ms, and a 429
//! retry storm makes the limit worse (spike verdict, 2026-07-16).

use hark_stt::SttError;

/// `hark-stt`'s transport mapping prefixes connect-class failures (DNS,
/// refused, unreachable, TLS setup) with this marker in `SttError::Http`.
/// That crate is frozen; the contract test below pins the string against its
/// live behavior so drift is caught at test time, not in production.
const CONNECT_CLASS_PREFIX: &str = "connect failed";

/// True only for `Timeout` and connect-class `Http`. Everything else (Auth,
/// RateLimited, Provider, BadAudio, and non-connect transport failures where
/// the request may have already reached the provider) is not retried.
pub fn should_retry(error: &SttError) -> bool {
    match error {
        SttError::Timeout { .. } => true,
        SttError::Http { detail, .. } => detail.starts_with(CONNECT_CLASS_PREFIX),
        SttError::Auth { .. }
        | SttError::RateLimited { .. }
        | SttError::BadAudio(_)
        | SttError::Provider { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> String {
        "test".to_string()
    }

    #[test]
    fn timeout_retries() {
        assert!(should_retry(&SttError::Timeout {
            provider: p(),
            configured_ms: 15_000
        }));
    }

    #[test]
    fn connect_class_http_retries() {
        assert!(should_retry(&SttError::Http {
            provider: p(),
            detail: "connect failed (no network, DNS, or provider down): dns error".to_string(),
        }));
    }

    #[test]
    fn non_connect_http_does_not_retry() {
        // Mid-body transport failures may have reached the provider already.
        assert!(!should_retry(&SttError::Http {
            provider: p(),
            detail: "error sending request: broken pipe".to_string(),
        }));
    }

    #[test]
    fn auth_never_retries() {
        assert!(!should_retry(&SttError::Auth { provider: p() }));
    }

    #[test]
    fn rate_limited_never_retries() {
        assert!(!should_retry(&SttError::RateLimited {
            provider: p(),
            retry_after_s: Some(2)
        }));
        assert!(!should_retry(&SttError::RateLimited {
            provider: p(),
            retry_after_s: None
        }));
    }

    #[test]
    fn bad_audio_never_retries() {
        assert!(!should_retry(&SttError::BadAudio("truncated".to_string())));
    }

    #[test]
    fn provider_error_never_retries() {
        assert!(!should_retry(&SttError::Provider {
            provider: p(),
            detail: "HTTP 500: oops".to_string(),
        }));
    }

    /// Contract test: pin the connect-class prefix against hark-stt's LIVE
    /// transport mapping. Connects to a loopback port that was just bound
    /// and released, so the refusal is local and instant; no external
    /// network is touched.
    #[test]
    fn connect_prefix_matches_hark_stt_contract() {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
            // Listener drops here; the port is now closed.
        };
        let client = hark_stt::shared_client().expect("client builds");
        let err = client
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .expect_err("connecting to a just-closed port must fail");
        let mapped = hark_stt::error_for_transport("test", 15_000, &err);
        match &mapped {
            SttError::Http { detail, .. } => {
                assert!(
                    detail.starts_with(CONNECT_CLASS_PREFIX),
                    "hark-stt connect mapping changed; update CONNECT_CLASS_PREFIX (got: {detail})"
                );
                assert!(should_retry(&mapped));
            }
            // Windows can surface an instant local refusal as a connect
            // timeout under some winsock configurations; that is also
            // retryable, so the contract holds either way.
            SttError::Timeout { .. } => assert!(should_retry(&mapped)),
            other => panic!("unexpected mapping for a connect failure: {other:?}"),
        }
    }
}
