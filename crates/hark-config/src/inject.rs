use serde::{Deserialize, Serialize};

/// How transcribed text reaches the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InjectStrategy {
    /// Clipboard stash -> set -> Ctrl+V -> restore (fast, default).
    Clipboard,
    /// Character-by-character typing (slower; paste-hostile fields).
    Type,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Inject {
    pub strategy: InjectStrategy,
    /// Delay between clipboard set and the synthesized paste. No OS-level
    /// guarantee exists; ~30-50 ms is the community rule of thumb, tuned on
    /// real hardware (spec §8).
    pub set_paste_delay_ms: u64,
    /// Delay between the synthesized paste and restoring the old clipboard.
    pub paste_restore_delay_ms: u64,
    /// Bounded retries when another process holds the clipboard open.
    pub clipboard_retries: u32,
}

impl Default for Inject {
    fn default() -> Self {
        Inject {
            strategy: InjectStrategy::Clipboard,
            set_paste_delay_ms: 50,
            paste_restore_delay_ms: 50,
            clipboard_retries: 8,
        }
    }
}
