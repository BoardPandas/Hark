//! The bundled test clip for the settings "Test connection" flow.
//!
//! The desktop app transcribes this clip through the user's configured
//! provider to validate auth, model name, and the full request path without
//! recording live audio. Embedded (not read from disk) because the release
//! binary must carry it; the spike example keeps reading the same file from
//! `fixtures/` so the two can never drift.
//!
//! The clip is ~10 s of 16 kHz mono speech (`fixtures/expected.txt` holds the
//! reference transcript used by the spike's Levenshtein check). Note for
//! Groq: 10 s is exactly its per-request billing minimum, so a test costs the
//! same as any short dictation.

/// Complete WAV bytes, ready for [`crate::SttProvider::transcribe`].
pub const SPIKE_WAV: &[u8] = include_bytes!("../fixtures/spike_clip.wav");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_fixture_matches_the_on_disk_clip() {
        let on_disk = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/spike_clip.wav"
        ))
        .expect("fixture file exists");
        assert_eq!(SPIKE_WAV, on_disk.as_slice());
    }
}
