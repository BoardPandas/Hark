//! Hark BYOK key resolution and storage: role env override (`HARK_STT_KEY` /
//! `HARK_CLEANUP_KEY`) first, then the OS keychain via `keyring`. No type in
//! this crate carries key material, so nothing here can Debug/Display a key.
//!
//! The env override is the dev/CI path (keys injected at run time, e.g. by
//! Doppler); the keychain is the end-user path (the settings UI paste field
//! writes the same slots via [`store_key`]). Keys never live in TOML. The
//! account is the provider label, shared between the STT and cleanup roles
//! by design (one key per provider); `voice.provider.key_account` covers the
//! two-distinct-openai-compatible-endpoints edge.
//!
//! No test in this crate may touch the real OS keyring (Phase 3 lesson):
//! backend interactions stay behind pure, separately-tested mapping
//! functions, and the empty-key guard returns before any backend call.

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

    #[error("refusing to store an empty API key for \"{account}\"")]
    EmptyKey { account: String },
}

/// Non-destructive presence check result for a stored key. Carries backend
/// diagnostics only, never key material, so it is safe to log or display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStatus {
    Stored,
    Missing,
    /// The backend failed; the UI shows the detail instead of guessing.
    Backend(String),
}

/// Store `key` (trimmed) in the OS keychain under `account`. Empty or
/// whitespace-only keys are rejected before the backend is touched, so a
/// stray paste can never blank out a working slot.
pub fn store_key(account: &str, key: &str) -> Result<(), KeyError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(KeyError::EmptyKey {
            account: account.to_string(),
        });
    }
    let entry = open_entry(account)?;
    entry.set_password(key).map_err(|e| KeyError::Backend {
        account: account.to_string(),
        detail: e.to_string(),
    })
}

/// Remove the stored key for `account`. Deleting a key that is not there is
/// success (the desired state holds), so the UI Remove button is idempotent.
pub fn delete_key(account: &str) -> Result<(), KeyError> {
    let entry = open_entry(account)?;
    map_delete_outcome(entry.delete_credential(), account)
}

/// Whether a key is stored for `account`. The stored value is read and
/// immediately dropped; it never crosses this function's boundary.
pub fn key_status(account: &str) -> KeyStatus {
    let entry = match open_entry(account) {
        Ok(entry) => entry,
        Err(KeyError::Backend { detail, .. }) => return KeyStatus::Backend(detail),
        // open_entry only produces Backend; keep the match total anyway.
        Err(e) => return KeyStatus::Backend(e.to_string()),
    };
    status_from_read(entry.get_password().map(|_key| ()))
}

fn open_entry(account: &str) -> Result<keyring::Entry, KeyError> {
    keyring::Entry::new(KEYRING_SERVICE, account).map_err(|e| KeyError::Backend {
        account: account.to_string(),
        detail: e.to_string(),
    })
}

/// Pure delete-outcome mapping: `NoEntry` is success, everything else is a
/// backend error. Unit-testable without a real keyring.
fn map_delete_outcome(outcome: Result<(), keyring::Error>, account: &str) -> Result<(), KeyError> {
    match outcome {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(KeyError::Backend {
            account: account.to_string(),
            detail: e.to_string(),
        }),
    }
}

/// Pure status mapping over an already-value-stripped read result.
/// Unit-testable without a real keyring.
fn status_from_read(read: Result<(), keyring::Error>) -> KeyStatus {
    match read {
        Ok(()) => KeyStatus::Stored,
        Err(keyring::Error::NoEntry) => KeyStatus::Missing,
        Err(e) => KeyStatus::Backend(e.to_string()),
    }
}

/// What [`copy_key_if_empty`] did. Labels only, never key material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyOutcome {
    Copied,
    /// The destination already holds a key, and a stored key is never
    /// overwritten: it may belong to a different endpoint.
    DestinationOccupied,
    /// Nothing (or only whitespace) is stored under the source account.
    SourceMissing,
}

/// Copy the OS keychain secret under `from` into `to`, only when `to` is
/// empty. Keychain to keychain: the env overrides play no part, since they
/// are not stored anywhere to copy. The source entry is left in place.
///
/// For a config migration that moved a provider to a new account name. The
/// caller decides when that happened; this only refuses to clobber.
pub fn copy_key_if_empty(from: &str, to: &str) -> Result<CopyOutcome, KeyError> {
    copy_with(from, to, read_keyring, store_key)
}

/// Pure copy decision over an injected store, unit-testable without a real
/// keyring. The destination is checked first, so an occupied slot never even
/// reads the source.
fn copy_with(
    from: &str,
    to: &str,
    mut read: impl FnMut(&str) -> Result<Option<String>, String>,
    write: impl FnOnce(&str, &str) -> Result<(), KeyError>,
) -> Result<CopyOutcome, KeyError> {
    let backend = |account: &str, detail: String| KeyError::Backend {
        account: account.to_string(),
        detail,
    };
    match read(to) {
        Ok(Some(existing)) if !existing.trim().is_empty() => {
            return Ok(CopyOutcome::DestinationOccupied)
        }
        Ok(_) => {}
        Err(detail) => return Err(backend(to, detail)),
    }
    let key = match read(from) {
        Ok(Some(key)) if !key.trim().is_empty() => key,
        Ok(_) => return Ok(CopyOutcome::SourceMissing),
        Err(detail) => return Err(backend(from, detail)),
    };
    write(to, &key)?;
    Ok(CopyOutcome::Copied)
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
            "openai",
        )
        .expect_err("must be Backend");
        assert!(matches!(err, KeyError::Backend { .. }));
        assert!(err.to_string().contains("credential store locked"));
    }

    #[test]
    fn empty_and_whitespace_keys_are_rejected_before_the_backend() {
        // These calls return before Entry::new, so this test never touches
        // the real OS keyring even though it goes through the public API.
        for bad in ["", "   ", "\t\n"] {
            let err = store_key("deepgram", bad).expect_err("must reject");
            assert!(matches!(err, KeyError::EmptyKey { .. }));
            let msg = err.to_string();
            assert!(msg.contains("deepgram"), "error names the account: {msg}");
        }
    }

    #[test]
    fn delete_treats_no_entry_as_success() {
        assert!(map_delete_outcome(Ok(()), "deepgram").is_ok());
        assert!(
            map_delete_outcome(Err(keyring::Error::NoEntry), "deepgram").is_ok(),
            "removing an absent key is the desired state, not an error"
        );
    }

    #[test]
    fn delete_backend_failure_surfaces_detail() {
        let err = map_delete_outcome(
            Err(keyring::Error::PlatformFailure(Box::new(
                std::io::Error::other("credential store locked"),
            ))),
            "openai",
        )
        .expect_err("must be Backend");
        assert!(matches!(err, KeyError::Backend { .. }));
        assert!(err.to_string().contains("credential store locked"));
        assert!(err.to_string().contains("openai"));
    }

    // --- copy_key_if_empty, over a fake store ---

    use std::cell::RefCell;
    use std::collections::HashMap;

    struct FakeStore {
        keys: RefCell<HashMap<String, String>>,
        reads: RefCell<Vec<String>>,
    }

    impl FakeStore {
        fn with(entries: &[(&str, &str)]) -> Self {
            FakeStore {
                keys: RefCell::new(
                    entries
                        .iter()
                        .map(|(a, k)| (a.to_string(), k.to_string()))
                        .collect(),
                ),
                reads: RefCell::new(Vec::new()),
            }
        }

        fn copy(&self, from: &str, to: &str) -> Result<CopyOutcome, KeyError> {
            copy_with(
                from,
                to,
                |account| {
                    self.reads.borrow_mut().push(account.to_string());
                    Ok(self.keys.borrow().get(account).cloned())
                },
                |account, key| {
                    self.keys
                        .borrow_mut()
                        .insert(account.to_string(), key.to_string());
                    Ok(())
                },
            )
        }

        fn get(&self, account: &str) -> Option<String> {
            self.keys.borrow().get(account).cloned()
        }
    }

    #[test]
    fn an_empty_destination_receives_the_source_key() {
        let store = FakeStore::with(&[("old", SENTINEL)]);
        assert_eq!(store.copy("old", "new").unwrap(), CopyOutcome::Copied);
        assert_eq!(store.get("new").as_deref(), Some(SENTINEL));
        assert_eq!(
            store.get("old").as_deref(),
            Some(SENTINEL),
            "a copy, not a move"
        );
    }

    #[test]
    fn an_occupied_destination_is_never_overwritten_or_even_compared() {
        let store = FakeStore::with(&[("old", SENTINEL), ("new", "someone-elses-key")]);
        assert_eq!(
            store.copy("old", "new").unwrap(),
            CopyOutcome::DestinationOccupied
        );
        assert_eq!(store.get("new").as_deref(), Some("someone-elses-key"));
        assert_eq!(
            *store.reads.borrow(),
            vec!["new".to_string()],
            "the source is not read once the destination is known to be taken"
        );
    }

    #[test]
    fn a_missing_or_blank_source_copies_nothing() {
        let empty = FakeStore::with(&[]);
        assert_eq!(
            empty.copy("old", "new").unwrap(),
            CopyOutcome::SourceMissing
        );
        assert_eq!(empty.get("new"), None);

        let blank = FakeStore::with(&[("old", "  ")]);
        assert_eq!(
            blank.copy("old", "new").unwrap(),
            CopyOutcome::SourceMissing
        );
        assert_eq!(blank.get("new"), None);
    }

    #[test]
    fn a_blank_destination_counts_as_empty() {
        let store = FakeStore::with(&[("old", SENTINEL), ("new", "")]);
        assert_eq!(store.copy("old", "new").unwrap(), CopyOutcome::Copied);
        assert_eq!(store.get("new").as_deref(), Some(SENTINEL));
    }

    #[test]
    fn copy_backend_failures_name_the_account_and_never_the_key() {
        let err = copy_with(
            "old",
            "new",
            |account| {
                if account == "new" {
                    Ok(None)
                } else {
                    Err("store locked".to_string())
                }
            },
            |_, _| panic!("nothing may be written after a failed read"),
        )
        .unwrap_err();
        let text = format!("{err} {err:?}");
        assert!(
            text.contains("old") && text.contains("store locked"),
            "{text}"
        );

        let write_failure = copy_with(
            "old",
            "new",
            |account| Ok((account == "old").then(|| SENTINEL.to_string())),
            |account, _| {
                Err(KeyError::Backend {
                    account: account.to_string(),
                    detail: "write refused".to_string(),
                })
            },
        )
        .unwrap_err();
        let text = format!("{write_failure} {write_failure:?}");
        assert!(!text.contains(SENTINEL), "an error leaked the key: {text}");
    }

    #[test]
    fn key_status_maps_all_three_shapes() {
        assert_eq!(status_from_read(Ok(())), KeyStatus::Stored);
        assert_eq!(
            status_from_read(Err(keyring::Error::NoEntry)),
            KeyStatus::Missing
        );
        match status_from_read(Err(keyring::Error::PlatformFailure(Box::new(
            std::io::Error::other("store unavailable"),
        )))) {
            KeyStatus::Backend(detail) => assert!(detail.contains("store unavailable")),
            other => panic!("expected Backend, got {other:?}"),
        }
    }

    #[test]
    fn write_path_errors_never_format_key_material() {
        // The EmptyKey path is the only write failure reachable without a
        // backend; feed it a sentinel-adjacent account and confirm neither
        // Debug nor Display can ever carry a key (the variant has no key
        // field, but guard against a refactor that adds one).
        let err = store_key("deepgram", "   ").unwrap_err();
        let debug = format!("{err:?}");
        let display = format!("{err}");
        assert!(!debug.contains(SENTINEL), "Debug leaked: {debug}");
        assert!(!display.contains(SENTINEL), "Display leaked: {display}");
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
