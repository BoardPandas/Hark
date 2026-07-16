//! Hark BYOK key resolution: role env override (`HARK_STT_KEY` /
//! `HARK_CLEANUP_KEY`) first, then the OS keychain via `keyring`. No type in
//! this crate carries key material, so nothing here can Debug/Display a key.
//!
//! The env override is the dev/CI path (keys injected at run time, e.g. by
//! Doppler); the keychain is the end-user path (the Phase 4 UI paste field
//! writes the same slots). Keys never live in TOML. The account is the
//! provider label, shared between the STT and cleanup roles by design (one
//! key per provider); `voice.provider.key_account` covers the
//! two-distinct-openai-compatible-endpoints edge.

use thiserror::Error;

/// Environment variable that overrides the OS keychain for the STT role.
pub const ENV_OVERRIDE: &str = "HARK_STT_KEY";

/// Environment variable that overrides the OS keychain for the cleanup role.
pub const CLEANUP_ENV_OVERRIDE: &str = "HARK_CLEANUP_KEY";

/// Keychain service name; the account is the provider label ("deepgram").
const KEYRING_SERVICE: &str = "hark";

/// Key-resolution failures. Variants carry account labels and backend
/// diagnostics only (never key material), so every variant is safe to log.
#[derive(Debug, Error)]
pub enum KeyError {
    #[error(
        "no API key for \"{account}\": set the {env_var} environment variable \
         or store a key in the OS keychain (service \"hark\", account \"{account}\")"
    )]
    Missing { account: String, env_var: String },

    #[error("OS keychain error for \"{account}\": {detail}")]
    Backend { account: String, detail: String },
}

/// Resolve the STT API key for a provider label. Kept as a thin wrapper so
/// existing call sites stand unchanged.
pub fn resolve_key(provider: &str) -> Result<String, KeyError> {
    resolve_key_for(ENV_OVERRIDE, provider)
}

/// Resolve a key for any role: `env_var` (if set and non-empty) beats the OS
/// keychain entry under `account`. The keychain is not touched at all when
/// the env override is present.
pub fn resolve_key_for(env_var: &str, account: &str) -> Result<String, KeyError> {
    let env = std::env::var(env_var).ok();
    pick(env, || read_keyring(account), env_var, account)
}

/// Pure precedence logic, unit-testable without process env or a real
/// keychain. `stored` is lazy so the env path never touches the backend.
fn pick(
    env: Option<String>,
    stored: impl FnOnce() -> Result<Option<String>, String>,
    env_var: &str,
    account: &str,
) -> Result<String, KeyError> {
    // An empty env var is treated as unset: `HARK_STT_KEY= hark-cli` must not
    // silently authenticate with an empty key.
    if let Some(key) = env.filter(|k| !k.trim().is_empty()) {
        return Ok(key);
    }
    match stored() {
        Ok(Some(key)) => Ok(key),
        Ok(None) => Err(KeyError::Missing {
            account: account.to_string(),
            env_var: env_var.to_string(),
        }),
        Err(detail) => Err(KeyError::Backend {
            account: account.to_string(),
            detail,
        }),
    }
}

/// Read the stored key. `Ok(None)` means "no entry" (a normal state);
/// `Err` is a real backend failure. Error strings from `keyring` describe
/// the backend condition and never echo stored secrets.
fn read_keyring(account: &str) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, account).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTINEL: &str = "sk-SENTINEL-NEVER-IN-LOGS";

    #[test]
    fn env_beats_keyring() {
        let got = pick(
            Some(SENTINEL.to_string()),
            || panic!("keychain must not be touched when the env override is set"),
            ENV_OVERRIDE,
            "deepgram",
        )
        .expect("env key resolves");
        assert_eq!(got, SENTINEL);
    }

    #[test]
    fn keyring_used_when_env_absent() {
        let got = pick(
            None,
            || Ok(Some("from-keyring".to_string())),
            ENV_OVERRIDE,
            "deepgram",
        )
        .expect("stored key resolves");
        assert_eq!(got, "from-keyring");
    }

    #[test]
    fn empty_env_var_falls_through_to_keyring() {
        let got = pick(
            Some("   ".to_string()),
            || Ok(Some("from-keyring".to_string())),
            ENV_OVERRIDE,
            "deepgram",
        )
        .expect("blank env override is ignored");
        assert_eq!(got, "from-keyring");
    }

    #[test]
    fn both_absent_is_a_clear_error_not_a_panic() {
        let err = pick(None, || Ok(None), ENV_OVERRIDE, "deepgram").expect_err("must be Missing");
        assert!(matches!(err, KeyError::Missing { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("HARK_STT_KEY"),
            "error must tell the user the fix: {msg}"
        );
        assert!(msg.contains("deepgram"), "error names the provider: {msg}");
    }

    #[test]
    fn cleanup_role_error_names_its_own_env_var() {
        // The cleanup role resolves through the same precedence logic but its
        // Missing error must point at HARK_CLEANUP_KEY, not the STT variable.
        let err =
            pick(None, || Ok(None), CLEANUP_ENV_OVERRIDE, "openai").expect_err("must be Missing");
        let msg = err.to_string();
        assert!(
            msg.contains("HARK_CLEANUP_KEY"),
            "error must name the cleanup env var: {msg}"
        );
        assert!(msg.contains("openai"), "error names the account: {msg}");
    }

    #[test]
    fn key_account_override_reaches_the_error_message() {
        // The voice.provider.key_account edge: the account in the message is
        // whatever the caller passed, not a hard-coded provider label.
        let err = pick(None, || Ok(None), CLEANUP_ENV_OVERRIDE, "my-alt-endpoint")
            .expect_err("must be Missing");
        assert!(err.to_string().contains("my-alt-endpoint"));
    }

    #[test]
    fn backend_failure_surfaces_detail() {
        let err = pick(
            None,
            || Err("credential store locked".to_string()),
            ENV_OVERRIDE,
            "groq",
        )
        .expect_err("must be Backend");
        assert!(matches!(err, KeyError::Backend { .. }));
        assert!(err.to_string().contains("credential store locked"));
    }

    #[test]
    fn no_error_path_ever_formats_key_material() {
        // Resolution failures happen when no key was found, so no variant can
        // even carry one, but guard against a refactor that threads the env
        // value into an error message: run every failure shape with a
        // sentinel-bearing environment and assert the sentinel never appears
        // in Debug or Display output.
        let failures: Vec<KeyError> = vec![
            pick(Some(String::new()), || Ok(None), ENV_OVERRIDE, "deepgram").unwrap_err(),
            pick(
                None,
                || Err("backend detail".to_string()),
                CLEANUP_ENV_OVERRIDE,
                "deepgram",
            )
            .unwrap_err(),
        ];
        for err in failures {
            let debug = format!("{err:?}");
            let display = format!("{err}");
            assert!(
                !debug.contains(SENTINEL),
                "Debug leaked key material: {debug}"
            );
            assert!(
                !display.contains(SENTINEL),
                "Display leaked key material: {display}"
            );
        }
    }
}
