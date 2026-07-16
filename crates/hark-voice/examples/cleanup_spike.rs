//! Phase 3 CP0 cleanup model spike (tasks/2026-07-16-phase3-voices.md).
//!
//! For each candidate chat model whose provider key is configured via env
//! (`OPENAI_API_KEY`, `GROQ_API_KEY`): rewrite fixture transcripts per voice
//! prompt (eyeball quality), measure warm latency (N runs, p50/p95), report
//! token usage vs our derived `max_completion_tokens` (reasoning headroom),
//! and run the verification drills the plan calls out: GPT-5 temperature
//! rejection, `reasoning_effort` acceptance on gpt-5-nano, the `{"error":...}`
//! envelope on forced 400/401, and `is_timeout()`/`is_connect()`
//! classification on the buffered-JSON transport path.
//!
//! Knobs: `SPIKE_RUNS` (default 10), `SPIKE_DELAY_MS` between timed runs
//! (default 0), `SPIKE_SKIP_DRILLS=1`.
//!
//! The voice prompt wording below is the CP0 tuning ground; CP2 lifts the
//! winning wording into `hark-voice` proper.

use hark_stt::metrics::{contains_term, LatencyTally};
use hark_stt::shared_client;
use hark_voice::openai_compatible::{
    build_request_body, chat_completions_url, max_completion_tokens, parse_response,
    retry_after_secs,
};
use hark_voice::{error_for_status, error_for_transport, CleanupError, CLEANUP_TIMEOUT_MS};
use reqwest::blocking::Client;
use std::time::{Duration, Instant};

/// Dictionary-ish protected terms; both appear in the long fixture.
const PROTECTED_TERMS: [&str; 2] = ["Hark", "Levenshtein"];

/// Below the default word gate (5) in production, but the spike rewrites it
/// anyway to see model behavior on trivial input.
const FIXTURE_SHORT: &str = "um okay send it";
const FIXTURE_MEDIUM: &str = "so um I think we should uh we should probably move the the \
    release to Friday you know because because the installer tests are still uh still flaky";
const FIXTURE_LONG: &str = "okay so um the way the way hark handles dictionary correction \
    is is basically a two pass thing right um first we run the the phonetic pass which uses \
    uh levenshtein distance on the on the phonetic codes and then um if the if the model \
    still mangles a term after cleanup we we just run the same pass again on the on the way \
    out um which sounds expensive but it's it's microseconds so so it's basically free \
    compared to the the network round trip you know";

/// One candidate model arm: provider endpoint + per-arm preset params.
struct ModelArm {
    provider: &'static str,
    base_url: &'static str,
    model: &'static str,
    temperature: Option<f32>,
    reasoning_effort: Option<&'static str>,
    api_key: String,
}

/// Collects every printed line and guarantees no configured API key ever
/// appears in the report (same discipline as transcribe_spike).
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

fn model_arms() -> Vec<ModelArm> {
    let mut arms = Vec::new();
    if let Some(key) = env_nonempty("OPENAI_API_KEY") {
        arms.push(ModelArm {
            provider: "openai",
            base_url: "https://api.openai.com/v1",
            model: "gpt-5-nano",
            temperature: None, // GPT-5 family locks temperature to the default
            reasoning_effort: Some("minimal"),
            api_key: key.clone(),
        });
        arms.push(ModelArm {
            provider: "openai",
            base_url: "https://api.openai.com/v1",
            model: "gpt-4.1-mini",
            temperature: Some(0.2),
            reasoning_effort: None,
            api_key: key,
        });
    }
    if let Some(key) = env_nonempty("GROQ_API_KEY") {
        arms.push(ModelArm {
            provider: "groq",
            base_url: "https://api.groq.com/openai/v1",
            model: "llama-3.1-8b-instant",
            temperature: Some(0.2),
            reasoning_effort: None,
            api_key: key.clone(),
        });
        arms.push(ModelArm {
            provider: "groq",
            base_url: "https://api.groq.com/openai/v1",
            model: "openai/gpt-oss-20b",
            temperature: Some(0.2),
            reasoning_effort: None,
            api_key: key,
        });
    }
    arms
}

/// Draft voice prompts (§2.2 shape: instruction, protected-terms clause for
/// terms present in the text, return-only close). Tuned here, lifted at CP2.
fn system_prompt(voice: &str, text: &str) -> String {
    let instruction = match voice {
        "clean" => {
            "Rewrite the transcript below. Fix punctuation, capitalization, filler words \
             (um, uh, you know), false starts, and repeated words. Preserve the original \
             wording, meaning, and tone. Never add or remove content."
        }
        "professional" => {
            "Rewrite the transcript below in a polished, professional business register \
             suitable for a written message to a colleague. Preserve the meaning; never \
             add or remove content."
        }
        "casual" => {
            "Rewrite the transcript below in a relaxed, casual conversational register. \
             Fix filler words and false starts but keep it informal. Preserve the meaning; \
             never add or remove content."
        }
        other => panic!("unknown spike voice {other}"),
    };
    let lower = text.to_lowercase();
    let present: Vec<&str> = PROTECTED_TERMS
        .iter()
        .filter(|t| lower.contains(&t.to_lowercase()))
        .copied()
        .collect();
    let mut prompt = instruction.to_string();
    if !present.is_empty() {
        prompt.push_str(&format!(
            " Leave these terms exactly as written: {}.",
            present.join(", ")
        ));
    }
    prompt
        .push_str(" Return only the rewritten text, with no commentary and no surrounding quotes.");
    prompt
}

struct HttpOutcome {
    status: u16,
    body: String,
    retry_after_s: Option<u64>,
    request_ms: u128,
}

/// One buffered-JSON POST with the production per-request timeout. Transport
/// errors come back raw so drills can inspect `is_timeout()`/`is_connect()`.
fn post_chat(
    client: &Client,
    url: &str,
    api_key: &str,
    body: Vec<u8>,
) -> Result<HttpOutcome, reqwest::Error> {
    let started = Instant::now();
    let response = client
        .post(url)
        .bearer_auth(api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .timeout(Duration::from_millis(CLEANUP_TIMEOUT_MS))
        .body(body)
        .send()?;
    let status = response.status().as_u16();
    let retry_after_s = retry_after_secs(response.headers());
    let body = response.text()?;
    Ok(HttpOutcome {
        status,
        body,
        retry_after_s,
        request_ms: started.elapsed().as_millis(),
    })
}

/// Full request -> parsed text, mapped through the production error taxonomy.
fn clean_once(
    client: &Client,
    arm: &ModelArm,
    voice: &str,
    text: &str,
) -> Result<(String, HttpOutcome), CleanupError> {
    let body = build_request_body(
        arm.model,
        &system_prompt(voice, text),
        text,
        arm.temperature,
        arm.reasoning_effort,
    );
    let outcome = post_chat(
        client,
        &chat_completions_url(arm.base_url),
        &arm.api_key,
        body,
    )
    .map_err(|e| error_for_transport(arm.provider, CLEANUP_TIMEOUT_MS, &e))?;
    if !(200..300).contains(&outcome.status) {
        return Err(error_for_status(
            arm.provider,
            outcome.status,
            outcome.retry_after_s,
            &outcome.body,
        ));
    }
    let cleaned = parse_response(arm.provider, &outcome.body)?;
    Ok((cleaned, outcome))
}

/// Token accounting from the raw body for the reasoning-headroom check.
fn usage_summary(body: &str, cap_sent: u32) -> String {
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return "usage: unparseable".into(),
    };
    format!(
        "cap {cap_sent}, completion_tokens {}, reasoning_tokens {}, finish_reason {}",
        v["usage"]["completion_tokens"],
        v["usage"]["completion_tokens_details"]["reasoning_tokens"],
        v["choices"][0]["finish_reason"]
    )
}

fn main() {
    let arms = model_arms();
    let report = Report {
        keys: arms.iter().map(|a| a.api_key.clone()).collect(),
    };
    let r = &report;

    r.say("=== Hark Phase 3 CP0 cleanup spike: BYOK chat models ===");
    for (env, provider) in [("OPENAI_API_KEY", "openai"), ("GROQ_API_KEY", "groq")] {
        if !arms.iter().any(|a| a.provider == provider) {
            r.say(format!("provider {provider}: skipped (no {env} set)"));
        }
    }
    if arms.is_empty() {
        r.say("no providers configured; set at least one key and re-run.");
        return;
    }

    let client = shared_client().expect("client builds");
    let runs: usize = env_nonempty("SPIKE_RUNS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let delay_ms: u64 = env_nonempty("SPIKE_DELAY_MS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // --- rewrite quality per voice (eyeball) + reasoning headroom ---
    let fixtures = [
        ("short", FIXTURE_SHORT),
        ("medium", FIXTURE_MEDIUM),
        ("long", FIXTURE_LONG),
    ];
    for arm in &arms {
        r.say(format!(
            "\n--- {}/{} (temperature {:?}, reasoning_effort {:?}) ---",
            arm.provider, arm.model, arm.temperature, arm.reasoning_effort
        ));
        // Clean on every fixture; register voices on medium only.
        let quality_calls: Vec<(&str, &str, &str)> = fixtures
            .iter()
            .map(|(name, text)| ("clean", *name, *text))
            .chain([
                ("professional", "medium", FIXTURE_MEDIUM),
                ("casual", "medium", FIXTURE_MEDIUM),
            ])
            .collect();
        for (voice, fixture_name, text) in quality_calls {
            let cap = max_completion_tokens(text);
            match clean_once(&client, arm, voice, text) {
                Ok((cleaned, outcome)) => {
                    r.say(format!(
                        "[{voice}/{fixture_name}] {} ms ({}): {:?}",
                        outcome.request_ms,
                        usage_summary(&outcome.body, cap),
                        cleaned
                    ));
                    if fixture_name == "long" {
                        let intact = PROTECTED_TERMS.iter().all(|t| contains_term(&cleaned, t));
                        r.say(format!("  protected terms intact: {intact}"));
                    }
                }
                Err(e) => r.say(format!("[{voice}/{fixture_name}] FAILED: {e}")),
            }
        }
    }

    // --- latency: N warm runs of clean/medium per arm ---
    r.say(format!(
        "\n--- latency (N={runs} warm runs of clean/medium, {delay_ms} ms inter-run delay) ---"
    ));
    r.say(format!(
        "{:<32} {:>7} {:>7} {:>7} {:>7} {:>6}",
        "provider/model", "p50", "p95", "min", "max", "errors"
    ));
    let mut measured: Vec<(String, LatencyTally)> = Vec::new();
    for arm in &arms {
        let name = format!("{}/{}", arm.provider, arm.model);
        let mut tally = LatencyTally::default();
        let mut errors = 0usize;
        for _ in 0..runs {
            match clean_once(&client, arm, "clean", FIXTURE_MEDIUM) {
                Ok((_, outcome)) => tally.record(outcome.request_ms),
                Err(e) => {
                    errors += 1;
                    r.say(format!("{name}: run error: {e}"));
                    if let CleanupError::RateLimited { retry_after_s, .. } = e {
                        std::thread::sleep(Duration::from_secs(retry_after_s.unwrap_or(2)));
                    }
                }
            }
            if delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(delay_ms));
            }
        }
        let fmt = |v: Option<u128>| v.map_or("-".to_string(), |ms| ms.to_string());
        r.say(format!(
            "{:<32} {:>7} {:>7} {:>7} {:>7} {:>6}",
            name,
            fmt(tally.p50()),
            fmt(tally.p95()),
            fmt(tally.min()),
            fmt(tally.max()),
            errors
        ));
        if tally.n() > 0 {
            measured.push((name, tally));
        }
    }

    // --- verification drills ---
    if env_nonempty("SPIKE_SKIP_DRILLS").is_none() {
        r.say("\n--- verification drills ---");

        // 1. GPT-5 temperature rejection: research says any non-default
        //    temperature is a 400. Also proves the error envelope on a real 400.
        if let Some(nano) = arms.iter().find(|a| a.model == "gpt-5-nano") {
            let body = build_request_body(
                nano.model,
                &system_prompt("clean", FIXTURE_SHORT),
                FIXTURE_SHORT,
                Some(0.2), // deliberately illegal on the GPT-5 family
                None,
            );
            let url = chat_completions_url(nano.base_url);
            match post_chat(&client, &url, &nano.api_key, body) {
                Ok(o) if o.status == 400 => r.say(format!(
                    "  gpt-5 temperature=0.2 -> 400 as expected; envelope: {}",
                    snippet(&o.body)
                )),
                Ok(o) => r.say(format!(
                    "  gpt-5 temperature=0.2 -> HTTP {} (EXPECTED 400; research wrong?): {}",
                    o.status,
                    snippet(&o.body)
                )),
                Err(e) => r.say(format!("  gpt-5 temperature drill transport error: {e}")),
            }
            // 2. reasoning_effort acceptance on nano (reports are inconsistent).
            match clean_once(&client, nano, "clean", FIXTURE_SHORT) {
                Ok(_) => r.say("  gpt-5-nano accepts reasoning_effort=minimal: OK (200)"),
                Err(e) => r.say(format!(
                    "  gpt-5-nano reasoning_effort=minimal REJECTED: {e}"
                )),
            }
        } else {
            r.say("  gpt-5 drills skipped (no OPENAI_API_KEY)");
        }

        // 3. Forced 401: bad key against a real endpoint (envelope shape check;
        //    429 cannot be forced reliably; organic ones are reported above).
        let template = &arms[0];
        let body = build_request_body(template.model, "sys", "hi", None, None);
        match post_chat(
            &client,
            &chat_completions_url(template.base_url),
            "sk-invalid-spike-drill",
            body,
        ) {
            Ok(o) => {
                let mapped =
                    error_for_status(template.provider, o.status, o.retry_after_s, &o.body);
                r.say(format!(
                    "  bad key -> HTTP {} -> {} ; envelope: {}",
                    o.status,
                    variant(&mapped),
                    snippet(&o.body)
                ));
            }
            Err(e) => r.say(format!("  bad-key drill transport error: {e}")),
        }

        // Forced 400: nonexistent model (second real-envelope data point).
        let body = build_request_body("hark-no-such-model", "sys", "hi", None, None);
        match post_chat(
            &client,
            &chat_completions_url(template.base_url),
            &template.api_key,
            body,
        ) {
            Ok(o) => r.say(format!(
                "  bad model -> HTTP {}; envelope: {}",
                o.status,
                snippet(&o.body)
            )),
            Err(e) => r.say(format!("  bad-model drill transport error: {e}")),
        }

        // 4. Transport classification on the buffered-JSON path: the multipart
        //    masking bug (LL-G HIGH) must NOT reproduce: is_timeout/is_connect
        //    must stay meaningful.
        for (what, base_url) in [
            ("unreachable DNS", "https://api.hark-spike.invalid/v1"),
            ("non-routable IP", "https://10.255.255.1/v1"),
        ] {
            let body = build_request_body("drill", "sys", "hi", None, None);
            let started = Instant::now();
            match post_chat(&client, &chat_completions_url(base_url), "drill-key", body) {
                Ok(o) => r.say(format!("  {what}: UNEXPECTED HTTP {}", o.status)),
                Err(e) => {
                    let mapped = error_for_transport("drill", CLEANUP_TIMEOUT_MS, &e);
                    r.say(format!(
                        "  {what}: is_timeout={} is_connect={} -> {} in {} ms",
                        e.is_timeout(),
                        e.is_connect(),
                        variant(&mapped),
                        started.elapsed().as_millis()
                    ));
                }
            }
        }
    } else {
        r.say("\n--- verification drills skipped (SPIKE_SKIP_DRILLS set) ---");
    }

    // --- verdict ---
    r.say("\n=== verdict ===");
    measured.sort_by_key(|(_, t)| t.p95().unwrap_or(u128::MAX));
    match measured.first() {
        Some((name, tally)) => r.say(format!(
            "fastest arm: {name} (p95 = {} ms over {} runs); pick the default on quality + latency together, not latency alone.",
            tally.p95().unwrap_or(u128::MAX),
            tally.n()
        )),
        None => r.say("no latency data collected; configure at least one provider key."),
    }
    r.say("CP0 exit: pin §2.4 default models/presets in tasks/2026-07-16-phase3-voices.md from these measurements and record the spike verdict there.");
    r.say("key-leak self-check: OK (no configured key appeared in any report line)");
}

fn snippet(body: &str) -> String {
    let trimmed: String = body.trim().chars().take(220).collect();
    trimmed
}

fn variant(e: &CleanupError) -> &'static str {
    match e {
        CleanupError::Http { .. } => "Http",
        CleanupError::Auth { .. } => "Auth",
        CleanupError::RateLimited { .. } => "RateLimited",
        CleanupError::Timeout { .. } => "Timeout",
        CleanupError::Provider { .. } => "Provider",
    }
}
