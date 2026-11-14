use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkey {
    /// "+"-separated chord; hold ALL keys to record, release any to stop.
    /// Parsed by `hark-hotkey` at startup (unknown names error there).
    pub ptt_key: String,
    /// Stop Caps/Scroll Lock toggling when the lock key is pressed as part of
    /// the chord. Turning this off restores the observe-only hook exactly, and
    /// exists because none of the suppression can be verified without real
    /// Windows hardware — a config line beats waiting for a build.
    pub swallow_lock_keys: bool,
}

impl Default for Hotkey {
    fn default() -> Self {
        Hotkey {
            // Confirmed with the user 2026-07-16: chord, not a single key.
            ptt_key: "LCtrl+LWin".to_string(),
            swallow_lock_keys: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{ConfigError, Settings};

    #[test]
    fn empty_ptt_key_is_invalid() {
        let err = Settings::from_toml("[hotkey]\nptt_key = \"  \"")
            .expect_err("blank chord must be rejected");
        assert!(matches!(err, ConfigError::Invalid(_)));
    }
}
