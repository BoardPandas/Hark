//! Phase 1 STT spike harness (tasks/2026-07-15-phase1-stt-spike.md).
//!
//! For each provider whose key is configured via env (`GROQ_API_KEY`,
//! `OPENAI_API_KEY`, `DEEPGRAM_API_KEY`): transcribe the committed fixture,
//! measure cold-vs-warm latency (N runs, p50/p95), run the Deepgram keyterm
//! A/B, drill the failure taxonomy, and print a default-provider + retry-policy
//! verdict. Providers without keys are skipped with an explicit message.
//!
//! Knobs: `SPIKE_RUNS` (default 20), `SPIKE_DELAY_MS` between timed runs
//! (default 0), `OPENAI_STT_MODEL` (default gpt-4o-mini-transcribe),
//! `GROQ_STT_MODEL` (default whisper-large-v3-turbo), `SPIKE_SKIP_DRILLS=1`.

use hark_stt::metrics::{contains_term, divergence_ratio, LatencyTally};
use hark_stt::{build, shared_client, wav, ProviderConfig, ProviderKind, SttError};
use std::time::{Duration, Instant};

/// Dictionary-ish terms spoken in the fixture; drive the prompt/keyterm biasing.
const BIAS_TERMS: [&str; 2] = ["Hark", "Levenshtein"];
/// The uncommon term the keyterm A/B watches for.
const AB_TERM: &str = "Levenshtein";

/// Collects every printed line and guarantees no configured API key ever
/// appears in the report (acceptance criterion 6, enforced at print time).
struct Report {
    keys: Vec<String>,
}

impl Report {
    fn say(&self, line: impl AsRef<str>) {
        let line = line.as_ref();
        for key in &self.keys {
            assert!(
                !key.is_empty() && !line.contains(key.as_str()),
                "SPIKE BUG: an API key almost reached the report; line suppressed"
            );
        }
        println!("{line}");
    }
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn provider_configs() -> Vec<ProviderConfig> {
    let bias = BIAS_TERMS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let mut configs = Vec::new();
    if let Some(key) = env_nonempty("GROQ_API_KEY") {
        configs.push(ProviderConfig {
            kind: ProviderKind::OpenAiCompatible,
            label: "groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            model: env_nonempty("GROQ_STT_MODEL")
                .unwrap_or_else(|| "whisper-large-v3-turbo".into()),
            api_key: key,
            bias_terms: bias.clone(),
        });
    }
    if let Some(key) = env_nonempty("OPENAI_API_KEY") {
        configs.push(ProviderConfig {
            kind: ProviderKind::OpenAiCompatible,
            label: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            model: env_nonempty("OPENAI_STT_MODEL")
                .unwrap_or_else(|| "gpt-4o-mini-transcribe".into()),
            api_key: key,
            bias_terms: bias.clone(),
        });
    }
    if let Some(key) = env_nonempty("DEEPGRAM_API_KEY") {
        configs.push(ProviderConfig {
            kind: ProviderKind::Deepgram,
            label: "deepgram".into(),
            base_url: "https://api.deepgram.com".into(),
            model: "nova-3".into(),
            api_key: key,
            bias_terms: bias.clone(),
        });
    }
    configs
}

fn main() {
    let fixture_dir = format!("{}/fixtures", env!("CARGO_MANIFEST_DIR"));
    let wav_bytes = std::fs::read(format!("{fixture_dir}/spike_clip.wav"))
        .expect("fixtures/spike_clip.wav must exist (committed)");
    let expected = std::fs::read_to_string(format!("{fixture_dir}/expected.txt"))
        .expect("fixtures/expected.txt must exist (committed)");

    // Validate the fixture before any request so provider results stay comparable.
    let info = wav::parse_wav_16k_mono(&wav_bytes).expect("fixture must be 16 kHz mono PCM16");
    let configs = provider_configs();
    let report = Report {
        keys: configs.iter().map(|c| c.api_key.clone()).collect(),
    };
    let r = &report;

    r.say("=== Hark Phase 1 STT spike: BYOK cloud adapters ===");
    r.say(format!(
        "fixture: {:.1} s, {} Hz, {} ch, {} bit; expected: {:?}",
        info.duration_secs(),
        info.sample_rate,
        info.channels,
        info.bits_per_sample,
        expected.trim()
    ));

    // WAV-encode cost, measured separately: the pipeline encodes from the ring
    // buffer on every utterance, so this belongs in the latency budget.
    let started = Instant::now();
    let reencoded = wav::encode_wav_16k_mono(&info.samples);
    let encode_us = started.elapsed().as_micros();
    r.say(format!(
        "wav encode ({} samples -> {} KiB): {:.2} ms",
        info.samples.len(),
        reencoded.len() / 1024,
        encode_us as f64 / 1000.0
    ));

    for name in ["groq", "openai", "deepgram"] {
        if !configs.iter().any(|c| c.label == name) {
            r.say(format!(
                "provider {name}: skipped (no {} set)",
                match name {
                    "groq" => "GROQ_API_KEY",
                    "openai" => "OPENAI_API_KEY",
                    _ => "DEEPGRAM_API_KEY",
                }
            ));
        }
    }
    if configs.is_empty() {
        r.say("no providers configured; set at least one key and re-run.");
        return;
    }

    let client = shared_client().expect("client builds");
    let runs: usize = env_nonempty("SPIKE_RUNS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let delay_ms: u64 = env_nonempty("SPIKE_DELAY_MS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // --- Checkpoint 1: one transcription per provider ---
    r.say("\n--- transcripts ---");
    for config in &configs {
        match build(config, client.clone()).and_then(|p| p.transcribe(&wav_bytes)) {
            Ok(t) => {
                r.say(format!(
                    "[{}/{}] {} ms, divergence {:.2}: {:?}",
                    config.label,
                    config.model,
                    t.request_ms,
                    divergence_ratio(&t.text, &expected),
                    t.text
                ));
            }
            Err(e) => r.say(format!("[{}/{}] FAILED: {e}", config.label, config.model)),
        }
    }

    // --- Checkpoint 2: latency, cold vs warm ---
    r.say(format!(
        "\n--- latency (cold run + N={runs} warm runs, {delay_ms} ms inter-run delay) ---"
    ));
    r.say(format!(
        "{:<22} {:>6} {:>7} {:>7} {:>7} {:>7} {:>7} {:>6}",
        "provider/model", "cold", "p50", "p95", "min", "max", "Δcold", "errors"
    ));
    let mut measured: Vec<(String, LatencyTally)> = Vec::new();
    for config in &configs {
        let name = format!("{}/{}", config.label, config.model);
        // Cold: a fresh client pays DNS + TCP + TLS on this one request.
        let cold_ms = shared_client()
            .and_then(|cold| build(config, cold)?.transcribe(&wav_bytes))
            .map(|t| t.request_ms);
        let provider = match build(config, client.clone()) {
            Ok(p) => p,
            Err(e) => {
                r.say(format!("{name}: adapter build failed: {e}"));
                continue;
            }
        };
        let mut tally = LatencyTally::default();
        let mut errors = 0usize;
        for _ in 0..runs {
            match provider.transcribe(&wav_bytes) {
                Ok(t) => tally.record(t.request_ms),
                Err(e) => {
                    errors += 1;
                    r.say(format!("{name}: run error: {e}"));
                    if let SttError::RateLimited { retry_after_s, .. } = e {
                        std::thread::sleep(Duration::from_secs(retry_after_s.unwrap_or(2)));
                    }
                }
            }
            if delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(delay_ms));
            }
        }
        let fmt = |v: Option<u128>| v.map_or("-".to_string(), |ms| ms.to_string());
        let delta = match (&cold_ms, tally.p50()) {
            (Ok(c), Some(p50)) => format!("{:+}", *c as i128 - p50 as i128),
            _ => "-".to_string(),
        };
        r.say(format!(
            "{:<22} {:>6} {:>7} {:>7} {:>7} {:>7} {:>7} {:>6}",
            name,
            cold_ms.as_ref().map_or("ERR".into(), |ms| ms.to_string()),
            fmt(tally.p50()),
            fmt(tally.p95()),
            fmt(tally.min()),
            fmt(tally.max()),
            delta,
            errors
        ));
        if tally.n() > 0 {
            measured.push((name, tally));
        }
    }

    // --- Checkpoint 3: Deepgram keyterm A/B ---
    if let Some(dg) = configs.iter().find(|c| c.kind == ProviderKind::Deepgram) {
        r.say(format!(
            "\n--- deepgram keyterm A/B (term: {AB_TERM:?}, 5 runs/arm) ---"
        ));
        for (arm, bias) in [
            ("without keyterm", vec![]),
            ("with keyterm", dg.bias_terms.clone()),
        ] {
            let config = ProviderConfig {
                bias_terms: bias,
                ..dg.clone()
            };
            let mut hits = 0;
            let mut sample = String::new();
            for i in 0..5 {
                match build(&config, client.clone()).and_then(|p| p.transcribe(&wav_bytes)) {
                    Ok(t) => {
                        if contains_term(&t.text, AB_TERM) {
                            hits += 1;
                        }
                        if i == 0 {
                            sample = t.text;
                        }
                    }
                    Err(e) => r.say(format!("  {arm}: run error: {e}")),
                }
            }
            r.say(format!(
                "  {arm}: {hits}/5 runs contained {AB_TERM:?}; sample: {sample:?}"
            ));
        }
    }

    // --- Checkpoint 4: failure drills ---
    if env_nonempty("SPIKE_SKIP_DRILLS").is_none() {
        r.say("\n--- failure drills ---");
        let template = configs
            .iter()
            .find(|c| c.kind == ProviderKind::OpenAiCompatible)
            .cloned()
            .unwrap_or_else(|| ProviderConfig {
                kind: ProviderKind::OpenAiCompatible,
                label: "drill".into(),
                base_url: "https://api.openai.com/v1".into(),
                model: "whisper-1".into(),
                api_key: String::new(),
                bias_terms: vec![],
            });
        let drills = [
            (
                "bad key -> Auth",
                ProviderConfig {
                    label: format!("{}-badkey", template.label),
                    api_key: "sk-invalid-spike-drill".into(),
                    ..template.clone()
                },
            ),
            (
                "unreachable DNS -> Http, bounded",
                ProviderConfig {
                    label: "drill-dns".into(),
                    base_url: "https://api.hark-spike.invalid/v1".into(),
                    ..template.clone()
                },
            ),
            (
                "non-routable IP -> Timeout/Http within connect bound",
                ProviderConfig {
                    label: "drill-blackhole".into(),
                    base_url: "https://10.255.255.1/v1".into(),
                    ..template.clone()
                },
            ),
        ];
        for (what, config) in drills {
            let started = Instant::now();
            let outcome = build(&config, client.clone()).and_then(|p| p.transcribe(&wav_bytes));
            let elapsed = started.elapsed().as_millis();
            match outcome {
                Ok(_) => r.say(format!("  {what}: UNEXPECTED SUCCESS ({elapsed} ms)")),
                Err(e) => r.say(format!(
                    "  {what}: {} in {elapsed} ms: {e}",
                    error_variant(&e)
                )),
            }
        }
    } else {
        r.say("\n--- failure drills skipped (SPIKE_SKIP_DRILLS set) ---");
    }

    // --- Verdict ---
    r.say("\n=== verdict ===");
    measured.sort_by_key(|(_, t)| t.p95().unwrap_or(u128::MAX));
    match measured.first() {
        Some((name, tally)) => {
            let p95 = tally.p95().unwrap_or(u128::MAX);
            r.say(format!(
                "default provider: {name} (lowest p95 = {p95} ms over {} runs)",
                tally.n()
            ));
            if p95 > 2_000 {
                r.say("⚠ p95 exceeds the ~2 s comfort bound for release-to-inject; consider re-testing on a better connection before locking the default.");
            }
        }
        None => r.say("no latency data collected; configure at least one provider key."),
    }
    r.say("retry policy proposal: one retry on Timeout or connect-class Http errors only; never retry 4xx (Auth/RateLimited surface to the user immediately).");
    r.say("cost caveats: Groq bills a 10 s minimum per request (short utterances cost as 10 s); Deepgram keyterm requires nova-3+; OpenAI gpt-4o-(mini-)transcribe is priced per audio minute.");
    r.say("key-leak self-check: OK (no configured key appeared in any report line)");
}

fn error_variant(e: &SttError) -> &'static str {
    match e {
        SttError::Http { .. } => "Http",
        SttError::Auth { .. } => "Auth",
        SttError::RateLimited { .. } => "RateLimited",
        SttError::Timeout { .. } => "Timeout",
        SttError::BadAudio(_) => "BadAudio",
        SttError::Provider { .. } => "Provider",
    }
}
