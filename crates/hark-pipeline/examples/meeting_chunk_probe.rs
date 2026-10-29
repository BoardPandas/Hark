//! Replay fixture clips of increasing length through the exact adapter a
//! meeting's live transcript uses, and report which ones finalise.
//!
//! Exists because meeting chunks (up to 30 s, replayed as one burst) stalled
//! where dictation replays (a few seconds) did not, and no offline harness can
//! show a server-side stall.
//!
//! ```sh
//! cargo run -p hark-pipeline --features live --example meeting_chunk_probe
//! ```
//!
//! Reads the provider and spellbook from `%APPDATA%\hark\config.toml` and the
//! key the same way the app does (keychain, or the env override). Neither the
//! key nor any transcript text is printed: only lengths, timings and errors.
//! Knobs: `PROBE_SECONDS` (default "10,20,30"), `PROBE_RUNS` (default 2).

use std::time::Instant;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn main() {
    let config = std::path::Path::new(&std::env::var("APPDATA").expect("APPDATA"))
        .join("hark")
        .join("config.toml");
    let settings = hark_config::Settings::load(&config).expect("config.toml loads");
    let label = settings.provider.kind.label().to_string();
    let key = hark_keychain::resolve_key(&label).expect("provider key resolves");
    let mut cfg = hark_pipeline::provider_config(&settings, key).expect("provider config");
    cfg.live_mode = hark_stt::gemini_live::TranscribeMode::Verbatim;
    let client = hark_stt::shared_client().expect("client");
    let provider = hark_stt::build_meeting_chunks(&cfg, client).expect("adapter");

    let fixture = format!(
        "{}/../hark-stt/fixtures/spike_clip.wav",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&fixture).expect("spike_clip.wav exists");
    let clip = hark_stt::wav::parse_wav_16k_mono(&bytes)
        .expect("16 kHz mono fixture")
        .samples;
    let seconds: Vec<usize> = env("PROBE_SECONDS")
        .unwrap_or_else(|| "10,20,30".into())
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let runs: usize = env("PROBE_RUNS").and_then(|v| v.parse().ok()).unwrap_or(2);

    println!("model {} ({} bias terms)", cfg.model, cfg.bias_terms.len());
    for &secs in &seconds {
        let samples: Vec<f32> = clip.iter().copied().cycle().take(secs * 16_000).collect();
        let wav = hark_stt::wav::encode_wav_16k_mono(&samples);
        for run in 1..=runs {
            let started = Instant::now();
            let outcome = match provider.transcribe(&wav) {
                Ok(t) => format!("ok, {} chars", t.text.chars().count()),
                Err(e) => format!("FAILED: {e}"),
            };
            println!(
                "{secs:>3} s clip, run {run}: {outcome} in {} ms",
                started.elapsed().as_millis()
            );
        }
    }
}
