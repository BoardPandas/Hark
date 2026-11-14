use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Audio {
    /// Which microphone to capture from, by cpal device name. `None` (or a
    /// name that no longer matches any present device) falls back to the OS
    /// default input device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_device: Option<String>,
    /// Audio kept from before the chord registers (catches led-in words).
    pub preroll_ms: u32,
    /// Audio kept after release (catches trailing word endings). Adds
    /// directly to release-to-inject latency; 150 ms confirmed 2026-07-16.
    pub tail_ms: u32,
    /// Max hold; on exceed, transcribe what we have (ring sized from this).
    pub max_hold_s: u32,
    /// Holds shorter than this are dropped without a network request.
    pub min_speech_ms: u32,
    /// Clips whose loudest 100 ms window stays below this RMS are dropped
    /// without a network request. Measured as a peak window, not a whole-clip
    /// mean, so the threshold means the same thing for a one-word command as
    /// for a long sentence.
    pub silence_rms: f32,
}

impl Default for Audio {
    fn default() -> Self {
        Audio {
            input_device: None,
            preroll_ms: 300,
            tail_ms: 150,
            max_hold_s: 120,
            min_speech_ms: 250,
            silence_rms: 0.01,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Settings;

    #[test]
    fn input_device_defaults_to_none_and_round_trips_when_set() {
        // Absent: the OS default microphone (serialized as nothing).
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert_eq!(s.audio.input_device, None);
        assert!(
            !Settings::default()
                .to_toml()
                .unwrap()
                .contains("input_device"),
            "the default (OS default mic) must not serialize a key"
        );

        // Present: the chosen device name parses and survives a save/load.
        let s = Settings::from_toml("[audio]\ninput_device = \"Yeti Stereo Microphone\"")
            .expect("named device parses");
        assert_eq!(
            s.audio.input_device.as_deref(),
            Some("Yeti Stereo Microphone")
        );
        let text = s.to_toml().expect("serializes");
        assert_eq!(
            Settings::from_toml(&text)
                .unwrap()
                .audio
                .input_device
                .as_deref(),
            Some("Yeti Stereo Microphone")
        );
    }
}
