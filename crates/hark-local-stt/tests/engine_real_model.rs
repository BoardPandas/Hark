//! End-to-end verification against the real weights.
//!
//! Ignored by default: it needs the ~670 MB model on disk and the `engine`
//! feature, neither of which belongs in a normal `cargo test`. This is the
//! test to run on real Windows/macOS hardware when validating a release.
//!
//! ```text
//! # 1. Download the model through the app (Settings -> On-device model), or
//! #    point at any directory holding the four model files.
//! # 2. Then:
//! HARK_LOCAL_MODEL_DIR=~/.local/share/hark/models/parakeet-tdt-0.6b-v3-int8 \
//!   cargo test -p hark-local-stt --features engine -- --ignored --nocapture
//! ```
//!
//! It prints load and decode wall times, which are the numbers that decide
//! whether on-device transcription is fast enough to be worth offering: the
//! whole product is release-to-inject latency.

use hark_local_stt::{LocalEngine, ModelStatus, PARAKEET_V3_INT8};
use std::path::PathBuf;

fn model_dir() -> Option<PathBuf> {
    match std::env::var_os("HARK_LOCAL_MODEL_DIR") {
        Some(d) => Some(PathBuf::from(d)),
        // Fall back to wherever the app would have put it.
        None => PARAKEET_V3_INT8.dir().ok(),
    }
}

/// Decode 16-bit PCM WAV bytes into the normalized f32 the engine expects.
/// Deliberately minimal: it only has to read the one fixture this crate's
/// sibling ships, not be a general WAV reader.
fn samples_from_wav(bytes: &[u8]) -> Vec<f32> {
    // Locate the `data` chunk rather than assuming a fixed 44-byte header.
    let pos = bytes
        .windows(4)
        .position(|w| w == b"data")
        .expect("fixture has a data chunk");
    let start = pos + 8;
    bytes[start..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32_768.0)
        .collect()
}

#[test]
#[ignore = "needs the ~670 MB model on disk and --features engine"]
fn transcribes_the_bundled_fixture_clip() {
    let Some(dir) = model_dir() else {
        panic!("no model directory; set HARK_LOCAL_MODEL_DIR");
    };
    assert_eq!(
        PARAKEET_V3_INT8.status_in(&dir),
        ModelStatus::Ready,
        "model at {} is not complete; download it first",
        dir.display()
    );

    let load_started = std::time::Instant::now();
    let engine = match LocalEngine::load(&PARAKEET_V3_INT8, &dir, 2) {
        Ok(e) => e,
        Err(e) => panic!("engine load failed: {e}"),
    };
    println!("model load: {} ms", load_started.elapsed().as_millis());

    let samples = samples_from_wav(hark_stt_fixture());
    let audio_ms = samples.len() * 1000 / 16_000;
    let transcript = match engine.transcribe(&samples) {
        Ok(t) => t,
        Err(e) => panic!("transcription failed: {e}"),
    };
    println!(
        "decoded {audio_ms} ms of audio in {} ms -> {} chars",
        transcript.request_ms,
        transcript.text.len()
    );
    println!("transcript: {}", transcript.text);

    assert!(
        !transcript.text.trim().is_empty(),
        "the fixture is ~10 s of clear speech; an empty transcript means the \
         model or the sample format is wrong"
    );
}

/// The same clip `hark-stt` uses for its "Test connection" flow, read from
/// its fixtures directory so the two can never drift.
fn hark_stt_fixture() -> &'static [u8] {
    include_bytes!("../../hark-stt/fixtures/spike_clip.wav")
}
