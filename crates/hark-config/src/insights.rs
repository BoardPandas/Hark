use serde::{Deserialize, Serialize};

/// Optional local Insights inputs. Both require explicit consent; enabling
/// text analysis never changes history capture or retention.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Insights {
    /// Collect the foreground app's name at dictation start, never titles.
    pub track_apps: bool,
    /// Analyze retained transcripts locally for words and repeated phrases.
    pub analyze_text: bool,
}

#[cfg(test)]
mod tests {
    use crate::Settings;

    #[test]
    fn insights_opt_ins_are_independent_and_survive_a_settings_roundtrip() {
        let settings = Settings::from_toml(
            "[history]\ncapture = false\n[insights]\ntrack_apps = true\nanalyze_text = true",
        )
        .unwrap();
        let encoded = settings.to_toml().unwrap();
        let restored = Settings::from_toml(&encoded).unwrap();
        assert!(restored.insights.track_apps);
        assert!(restored.insights.analyze_text);
        assert!(
            !restored.history.capture,
            "opt-ins must never enable text storage"
        );
        let partial = Settings::from_toml("[insights]\ntrack_apps = true").unwrap();
        assert!(partial.insights.track_apps);
        assert!(!partial.insights.analyze_text);
    }
}
