//! Gemini fused-adapter spike: does one round trip that returns
//! `{raw, cleaned}` beat the current two-hop path (STT POST, then cleanup POST)
//! without giving up the expansion guardrail?
//!
//! Run with `GEMINI_API_KEY` set. If `OPENAI_API_KEY` or `GROQ_API_KEY` is also
//! set, the spike measures the two-hop baseline on the same fixture so the
//! latency verdict is a comparison rather than a number in isolation.
//!
//! Knobs: `SPIKE_RUNS` (default 10), `SPIKE_DELAY_MS` (default 0),
//! `GEMINI_MODEL` (default gemini-3.6-flash), `GEMINI_BASE_URL`,
//! `SPIKE_SKIP_DRILLS=1`.

use hark_stt::gemini::Gemini;
use hark_stt::metrics::{contains_term, divergence_ratio, LatencyTally};
use hark_stt::{build, shared_client, wav, ProviderConfig, ProviderKind, SttError};
use hark_voice::openai_compatible::{CleanupConfig, OpenAiCompatibleChat};
use hark_voice::{over_expanded, system_prompt, CleanupProvider, Voice};
use std::time::{Duration, Instant};

/// Dictionary-ish terms spoken in the fixture, shared with the STT spike.
const BIAS_TERMS: [&str; 2] = ["Hark", "Levenshtein"];
const AB_TERM: &str = "Levenshtein";
/// The ratio the app ships with; the guardrail verdict below uses it.
const MAX_EXPANSION_RATIO: f32 = 1.5;
const DEFAULT_MODEL: &str = "gemini-3.6-flash";
const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Collects every printed line and guarantees no configured API key ever
/// appears in the report (same discipline as the STT spike).
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

fn main() {
    let fixture_dir = format!("{}/fixtures", env!("CARGO_MANIFEST_DIR"));
    let wav_bytes = std::fs::read(format!("{fixture_dir}/spike_clip.wav"))
        .expect("fixtures/spike_clip.wav must exist (committed)");
    let expected = std::fs::read_to_string(format!("{fixture_dir}/expected.txt"))
        .expect("fixtures/expected.txt must exist (committed)");
    let info = wav::parse_wav_16k_mono(&wav_bytes).expect("fixture must be 16 kHz mono PCM16");

    let Some(gemini_key) = env_nonempty("GEMINI_API_KEY") else {
        println!("GEMINI_API_KEY not set; nothing to spike.");
        return;
    };
    let baseline_key = env_nonempty("OPENAI_API_KEY").map(|k| ("openai", k));
    let baseline_key = baseline_key.or_else(|| env_nonempty("GROQ_API_KEY").map(|k| ("groq", k)));

    let report = Report {
        keys: std::iter::once(gemini_key.clone())
            .chain(baseline_key.iter().map(|(_, k)| k.clone()))
            .collect(),
    };
    let r = &report;

    let bias: Vec<String> = BIAS_TERMS.iter().map(|s| s.to_string()).collect();
    let model = env_nonempty("GEMINI_MODEL").unwrap_or_else(|| DEFAULT_MODEL.into());
    let base_url = env_nonempty("GEMINI_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.into());

    // The fused call has no outgoing text to subset the dictionary against —
    // the transcript does not exist yet — so every term is treated as present.
    // Two-hop cleanup can subset; the fused prompt is therefore slightly larger.
    let all_terms: Vec<&str> = bias.iter().map(String::as_str).collect();
    let cleanup_instruction =
        system_prompt(Voice::Clean, "", &all_terms).expect("Clean is not Verbatim");

    let fused_config = ProviderConfig {
        kind: ProviderKind::Gemini,
        label: "gemini".into(),
        base_url: base_url.clone(),
        model: model.clone(),
        api_key: gemini_key.clone(),
        bias_terms: bias.clone(),
        cleanup_instruction: Some(cleanup_instruction),
    };
    // Same provider with cleanup off, to see whether the fused prompt is what
    // makes the transcript drift or whether the model paraphrases regardless.
    let transcribe_only = ProviderConfig {
        cleanup_instruction: None,
        ..fused_config.clone()
    };

    r.say("=== Hark Gemini fused-adapter spike ({raw, cleaned} in one round trip) ===");
    r.say(format!(
        "fixture: {:.1} s, {} Hz, {} ch; expected: {:?}",
        info.duration_secs(),
        info.sample_rate,
        info.channels,
        expected.trim()
    ));
    r.say(format!("model: {model} @ {base_url}"));

    let client = shared_client().expect("client builds");
    let runs: usize = env_nonempty("SPIKE_RUNS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let delay_ms: u64 = env_nonempty("SPIKE_DELAY_MS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // --- Checkpoint 1: the pair, and whether the guardrail still bites ---
    r.say("\n--- fused output ---");
    let fused_adapter = Gemini::new(&fused_config, client.clone());
    match fused_adapter.transcribe_and_clean(&wav_bytes) {
        Ok((fused, ms)) => {
            r.say(format!("[{ms} ms] raw:     {:?}", fused.raw));
            r.say(format!(
                "         cleaned: {:?}",
                fused.cleaned.as_deref().unwrap_or("<unchanged>")
            ));
            r.say(format!(
                "         divergence(raw vs expected): {:.2}; keyterm {AB_TERM:?} present: {}",
                divergence_ratio(&fused.raw, &expected),
                contains_term(&fused.raw, AB_TERM)
            ));
            match fused.cleaned.as_deref() {
                Some(cleaned) => {
                    let over = over_expanded(&fused.raw, cleaned, MAX_EXPANSION_RATIO);
                    r.say(format!(
                        "         expansion guard ({} -> {} words, ratio {MAX_EXPANSION_RATIO}): {}",
                        fused.raw.split_whitespace().count(),
                        cleaned.split_whitespace().count(),
                        if over {
                            "TRIPPED (would inject raw)"
                        } else {
                            "ok"
                        }
                    ));
                }
                None => r.say("         expansion guard: n/a (cleanup was a no-op)"),
            }
        }
        Err(e) => r.say(format!("FAILED: {e}")),
    }

    r.say("\n--- transcribe-only (same model, no cleanup instruction) ---");
    match build(&transcribe_only, client.clone()).and_then(|p| p.transcribe(&wav_bytes)) {
        Ok(t) => r.say(format!(
            "[{} ms] divergence {:.2}: {:?}",
            t.request_ms,
            divergence_ratio(&t.text, &expected),
            t.text
        )),
        Err(e) => r.say(format!("FAILED: {e}")),
    }

    // --- Checkpoint 2: fidelity across runs ---
    // One sample proves nothing about an LLM doing ASR: the question is whether
    // the transcript is *stable*, since a paraphrase that appears one run in
    // five is worse than one that appears every time (it passes a spot check).
    r.say(format!("\n--- transcript stability ({runs} runs) ---"));
    let mut transcripts: Vec<String> = Vec::new();
    let mut tripped = 0usize;
    for _ in 0..runs {
        match fused_adapter.transcribe_and_clean(&wav_bytes) {
            Ok((fused, _)) => {
                if let Some(cleaned) = fused.cleaned.as_deref() {
                    if over_expanded(&fused.raw, cleaned, MAX_EXPANSION_RATIO) {
                        tripped += 1;
                    }
                }
                transcripts.push(fused.raw);
            }
            Err(e) => r.say(format!("  run error: {e}")),
        }
        if delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
    }
    let mut distinct = transcripts.clone();
    distinct.sort();
    distinct.dedup();
    r.say(format!(
        "  {} distinct raw transcripts over {} successful runs",
        distinct.len(),
        transcripts.len()
    ));
    for variant in distinct.iter().take(5) {
        r.say(format!("    {variant:?}"));
    }
    r.say(format!(
        "  expansion guard tripped on {tripped}/{} runs",
        transcripts.len()
    ));

    // --- Checkpoint 3: latency, fused vs the two-hop baseline ---
    r.say(format!("\n--- latency (cold + {runs} warm runs) ---"));
    r.say(format!(
        "{:<28} {:>6} {:>7} {:>7} {:>7} {:>6}",
        "path", "cold", "p50", "p95", "max", "errors"
    ));

    let cold_fused = shared_client()
        .map(|cold| Gemini::new(&fused_config, cold))
        .and_then(|a| a.transcribe_and_clean(&wav_bytes).map(|(_, ms)| ms));
    let mut fused_tally = LatencyTally::default();
    let mut fused_errors = 0usize;
    for _ in 0..runs {
        match fused_adapter.transcribe_and_clean(&wav_bytes) {
            Ok((_, ms)) => fused_tally.record(ms),
            Err(e) => {
                fused_errors += 1;
                if let SttError::RateLimited { retry_after_s, .. } = e {
                    std::thread::sleep(Duration::from_secs(retry_after_s.unwrap_or(2)));
                }
            }
        }
        if delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
    }
    say_row(
        r,
        "gemini fused (1 hop)",
        &cold_fused,
        &fused_tally,
        fused_errors,
    );

    let mut baseline_tally = LatencyTally::default();
    match &baseline_key {
        Some((label, key)) => {
            let stt_config = ProviderConfig {
                kind: ProviderKind::OpenAiCompatible,
                label: (*label).into(),
                base_url: if *label == "groq" {
                    "https://api.groq.com/openai/v1".into()
                } else {
                    "https://api.openai.com/v1".into()
                },
                model: if *label == "groq" {
                    "whisper-large-v3-turbo".into()
                } else {
                    "gpt-4o-mini-transcribe".into()
                },
                api_key: key.clone(),
                bias_terms: bias.clone(),
                cleanup_instruction: None,
            };
            let cleanup_config = CleanupConfig {
                label: (*label).into(),
                base_url: stt_config.base_url.clone(),
                model: if *label == "groq" {
                    "llama-3.1-8b-instant".into()
                } else {
                    "gpt-5-nano".into()
                },
                api_key: key.clone(),
                temperature: None,
                reasoning_effort: None,
                voice: Voice::Clean,
                custom_prompt: String::new(),
                dictionary_terms: bias.clone(),
            };
            let stt = build(&stt_config, client.clone());
            let cleanup = OpenAiCompatibleChat::new(&cleanup_config, client.clone());
            let mut errors = 0usize;
            match (stt, cleanup) {
                (Ok(stt), Ok(cleanup)) => {
                    for _ in 0..runs {
                        // The real two-hop cost: transcription, then a cleanup
                        // call that cannot start until the transcript exists.
                        let started = Instant::now();
                        let outcome = stt
                            .transcribe(&wav_bytes)
                            .map_err(|e| e.to_string())
                            .and_then(|t| {
                                cleanup
                                    .clean(&t.text)
                                    .map_err(|e| e.to_string())
                                    .map(|_| ())
                            });
                        match outcome {
                            Ok(()) => baseline_tally.record(started.elapsed().as_millis()),
                            Err(_) => errors += 1,
                        }
                        if delay_ms > 0 {
                            std::thread::sleep(Duration::from_millis(delay_ms));
                        }
                    }
                    say_row(
                        r,
                        &format!("{label} stt+cleanup (2 hops)"),
                        &Err(()),
                        &baseline_tally,
                        errors,
                    );
                }
                _ => r.say("  baseline: adapter build failed"),
            }
        }
        None => r.say("  baseline: skipped (set OPENAI_API_KEY or GROQ_API_KEY to compare)"),
    }

    // --- Checkpoint 4: failure taxonomy ---
    if env_nonempty("SPIKE_SKIP_DRILLS").is_none() {
        r.say("\n--- failure drills ---");
        let drills = [
            (
                "bad key -> Auth",
                ProviderConfig {
                    label: "gemini-badkey".into(),
                    api_key: "not-a-real-gemini-key".into(),
                    ..fused_config.clone()
                },
            ),
            (
                "unreachable DNS -> Http, bounded",
                ProviderConfig {
                    label: "gemini-dns".into(),
                    base_url: "https://gemini.hark-spike.invalid/v1beta".into(),
                    ..fused_config.clone()
                },
            ),
            (
                "non-routable IP -> Timeout/Http within connect bound",
                ProviderConfig {
                    label: "gemini-blackhole".into(),
                    base_url: "https://10.255.255.1/v1beta".into(),
                    ..fused_config.clone()
                },
            ),
        ];
        for (what, config) in drills {
            let started = Instant::now();
            let outcome = build(&config, client.clone()).and_then(|p| p.transcribe(&wav_bytes));
            let elapsed = started.elapsed().as_millis();
            match outcome {
                Ok(_) => r.say(format!("  {what}: UNEXPECTED SUCCESS ({elapsed} ms)")),
                Err(e) => r.say(format!("  {what}: {} in {elapsed} ms: {e}", variant(&e))),
            }
        }
    }

    // --- Verdict ---
    r.say("\n=== verdict ===");
    match (fused_tally.p50(), baseline_tally.p50()) {
        (Some(fused), Some(base)) => {
            let delta = base as i128 - fused as i128;
            r.say(format!(
                "fused p50 {fused} ms vs two-hop p50 {base} ms: {}{} ms",
                if delta >= 0 { "saves " } else { "costs " },
                delta.abs()
            ));
        }
        (Some(fused), None) => r.say(format!(
            "fused p50 {fused} ms; no baseline measured, so this is not yet a comparison."
        )),
        _ => r.say("no fused latency collected."),
    }
    if let Some(p95) = fused_tally.p95() {
        if p95 > 2_000 {
            r.say("⚠ fused p95 exceeds the ~2 s release-to-inject comfort bound.");
        }
    }
    r.say(format!(
        "fidelity: {} distinct transcripts / {} runs, guard tripped {tripped}x. More than one \
         distinct transcript means the model is paraphrasing, which no ASR does — weigh that \
         against the latency saving before shipping this as the default.",
        distinct.len(),
        transcripts.len()
    ));
    r.say(
        "open questions: temperature and thinking-level controls are not wired (unknown field \
           names on the Interactions API would 400 the whole call); both are likely levers on \
           fidelity and latency respectively.",
    );
    r.say("key-leak self-check: OK (no configured key appeared in any report line)");
}

fn say_row(
    r: &Report,
    name: &str,
    cold: &Result<u128, impl std::fmt::Debug>,
    tally: &LatencyTally,
    errors: usize,
) {
    let fmt = |v: Option<u128>| v.map_or("-".to_string(), |ms| ms.to_string());
    r.say(format!(
        "{:<28} {:>6} {:>7} {:>7} {:>7} {:>6}",
        name,
        cold.as_ref().map_or("-".to_string(), |ms| ms.to_string()),
        fmt(tally.p50()),
        fmt(tally.p95()),
        fmt(tally.max()),
        errors
    ));
}

fn variant(e: &SttError) -> &'static str {
    match e {
        SttError::Http { .. } => "Http",
        SttError::Auth { .. } => "Auth",
        SttError::RateLimited { .. } => "RateLimited",
        SttError::Timeout { .. } => "Timeout",
        SttError::BadAudio(_) => "BadAudio",
        SttError::Provider { .. } => "Provider",
    }
}
