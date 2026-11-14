use serde::{Deserialize, Serialize};

/// The `[history]` section: local capture + retention. Content capture and
/// numeric stats are deliberately independent: with `capture = false` no
/// dictation text is persisted, but the counters in the stats row still
/// tick (they carry no content).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    /// `false`: no dictation content is stored; counters still tick.
    pub capture: bool,
    /// Keep at most this many newest entries (>= 1).
    pub max_entries: u32,
    /// Delete entries older than this many days (>= 1).
    pub max_age_days: u32,
}

impl Default for History {
    fn default() -> Self {
        History {
            // Retention defaults confirmed with the user 2026-07-16:
            // 1,000 entries or 90 days, whichever prunes first.
            capture: true,
            max_entries: 1_000,
            max_age_days: 90,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{ConfigError, Settings};

    #[test]
    fn history_defaults_match_the_confirmed_retention() {
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert!(s.history.capture);
        assert_eq!(s.history.max_entries, 1_000);
        assert_eq!(s.history.max_age_days, 90);
    }

    #[test]
    fn history_section_parses_and_zero_caps_are_rejected() {
        let s =
            Settings::from_toml("[history]\ncapture = false\nmax_entries = 50\nmax_age_days = 7")
                .expect("history section parses");
        assert!(!s.history.capture);
        assert_eq!(s.history.max_entries, 50);
        assert_eq!(s.history.max_age_days, 7);

        for bad in ["[history]\nmax_entries = 0", "[history]\nmax_age_days = 0"] {
            let err = Settings::from_toml(bad).expect_err("zero caps must be rejected");
            assert!(matches!(err, ConfigError::Invalid(_)), "{bad}");
        }
    }
}
