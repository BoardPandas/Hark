//! Post progressively longer clips to an OpenAI-compatible endpoint and report
//! where it starts failing.
//!
//! Exists because a dictation-length-dependent failure is invisible to every
//! other harness here: the committed fixture is 10 s, so a defect that only
//! appears at paragraph length never shows up in the spike, the settings
//! "Test connection", or any unit test.
//!
//! ```sh
//! OPENAI_API_KEY=... cargo run -p hark-stt --example long_clip_probe
//! ```
//!
//! Knobs: `PROBE_MODEL` (default gpt-transcribe), `PROBE_SECONDS` (default
//! "10,30,60,120,300"), `PROBE_TERMS` (default 2 bias terms; set 0 to send
//! none, to separate a body-size problem from a field-count one).

use hark_stt::{wav, ProviderConfig, ProviderKind};

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn main() {
    let Some(key) = env("OPENAI_API_KEY") else {
        eprintln!("set OPENAI_API_KEY to run this probe");
        std::process::exit(2);
    };
    let model = env("PROBE_MODEL").unwrap_or_else(|| "gpt-transcribe".into());
    let seconds: Vec<u32> = env("PROBE_SECONDS")
        .unwrap_or_else(|| "10,30,60,120,300".into())
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let term_count: usize = env("PROBE_TERMS").and_then(|v| v.parse().ok()).unwrap_or(2);

    let fixture = format!("{}/fixtures/spike_clip.wav", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&fixture).expect("fixtures/spike_clip.wav must exist");
    let info = wav::parse_wav_16k_mono(&bytes).expect("fixture must be 16 kHz mono PCM16");

    // Synthesised past the two real ones: the question is how many
    // `keywords[]` fields the endpoint tolerates, not what they say.
    let terms: Vec<String> = (0..term_count)
        .map(|i| match i {
            0 => "Hark".to_string(),
            1 => "Levenshtein".to_string(),
            n => format!("spellbookterm{n}"),
        })
        .collect();

    println!("model: {model}  bias terms: {}", terms.len());
    println!("{:>6}  {:>10}  {:>8}  result", "secs", "bytes", "ms");

    let client = hark_stt::shared_client().expect("client");
    for want in seconds {
        // Repeat the fixture to the requested length: real speech, so the
        // provider is doing real work rather than transcribing silence.
        let want_samples = (want * 16_000) as usize;
        let mut samples = Vec::with_capacity(want_samples);
        while samples.len() < want_samples {
            samples.extend_from_slice(&info.samples);
        }
        samples.truncate(want_samples);
        let clip = wav::encode_wav_16k_mono(&samples);

        let config = ProviderConfig {
            kind: ProviderKind::OpenAiTranscribe,
            label: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            model: model.clone(),
            api_key: key.clone(),
            bias_terms: terms.clone(),
            cleanup_instruction: None,
            live_mode: hark_stt::gemini_live::TranscribeMode::Verbatim,
        };
        let adapter = hark_stt::build(&config, client.clone()).expect("adapter builds");
        let started = std::time::Instant::now();
        let outcome = match adapter.transcribe(&clip) {
            Ok(t) => format!("ok, {} chars", t.text.chars().count()),
            Err(e) => format!("FAILED: {e}"),
        };
        println!(
            "{want:>6}  {:>10}  {:>8}  {outcome}",
            clip.len(),
            started.elapsed().as_millis()
        );
    }
}
