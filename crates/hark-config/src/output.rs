use serde::{Deserialize, Serialize};

/// Post-transcription text shaping applied just before injection, after any
/// cleanup pass. Independent of the STT provider and of cleanup, so it behaves
/// the same whether or not a voice ran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Output {
    /// Drop the trailing period on a single-word dictation ("Hello." ->
    /// "Hello"). A lone word is rarely a sentence, so the period the STT
    /// provider or a cleanup voice appends is usually noise the user did not
    /// ask for. Default on; the setting exists to turn it back off.
    pub strip_single_word_period: bool,
}

impl Default for Output {
    fn default() -> Self {
        Output {
            strip_single_word_period: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Settings;

    #[test]
    fn output_defaults_to_stripping_single_word_periods_and_round_trips() {
        // Default on: a fresh install and a config file predating [output]
        // both strip the lone-word period until the user opts out.
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert!(s.output.strip_single_word_period);

        let s = Settings::from_toml("[output]\nstrip_single_word_period = false")
            .expect("output section parses");
        assert!(!s.output.strip_single_word_period);
        let text = s.to_toml().expect("serializes");
        assert!(
            !Settings::from_toml(&text)
                .unwrap()
                .output
                .strip_single_word_period
        );
    }
}
