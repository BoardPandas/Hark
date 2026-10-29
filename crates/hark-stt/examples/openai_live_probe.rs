//! Probe OpenAI's realtime transcription socket (`gpt-live-transcribe`)
//! against the real API, before any adapter trusts the documented contract
//! (CP0 of `tasks/2026-10-29-plan-openai-live.md`).
//!
//! Gemini Live shipped with three wire bugs that only a live probe found
//! (LL-G Rust HIGH, `websocket-binary-frames-not-text`), so this prints frames
//! raw, with their type and timing, then works through the questions the docs
//! leave open and ends with a summary:
//!
//! - text or binary frames, and whether deltas arrive before the commit
//! - commit-to-transcript latency for each `delay`, against gpt-transcribe
//!   batch on the same clip
//! - the shortest clip a commit accepts
//! - what a 16 kHz rate and a bad key are told
//!
//! Then it runs the same fixture through the real adapter, both ways the app
//! does: replayed through `hark_stt::build` (Test connection, and the fallback
//! when streaming drops), and streamed at real time through
//! `hark_stt::build_live` the way the pump feeds it during a hold.
//!
//! ```sh
//! OPENAI_API_KEY=... cargo run -p hark-stt --features live \
//!     --example openai_live_probe
//! ```
//!
//! Knobs: `PROBE_WAV=<path>` sends your own 16 kHz mono clip instead of the
//! fixture, `PROBE_BURST=1` sends audio as fast as possible instead of at real
//! time, `PROBE_VERBOSE=1` dumps raw frames for every turn rather than the
//! first, `PROBE_IDLE_SECS=<n>` also holds one socket open for n seconds
//! with no audio, for checking idle billing on the usage page, and
//! `PROBE_NO_WAIT=1` appends straight after `session.update` without waiting
//! for `session.updated`, which is what the adapter does (D10).
//!
//! The key travels in the Authorization header, marked sensitive, and nothing
//! here prints the request. OpenAI's auth errors quote a masked key, so every
//! server string is scrubbed before it is printed.

use futures_util::{SinkExt, StreamExt};
use hark_stt::gemini_live::TranscribeMode;
use hark_stt::openai_live::{
    append_message, commit_message, realtime_url, scrub, upsample_16k_to_24k, INPUT_RATE,
};
use hark_stt::wav::{self, samples_to_pcm16_le};
use hark_stt::{ProviderConfig, ProviderKind};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The OpenAI provider's default base URL; the adapter derives the socket URL.
const BASE_URL: &str = "https://api.openai.com/v1";
const MODEL: &str = "gpt-live-transcribe";
/// One append per 100 ms of audio, the cadence the adapter sends at.
const CHUNK_MS: usize = 100;
/// The streaming pump's wake-up interval, for the real-time adapter session.
const PUMP_MS: u64 = 20;
const BIAS_TERMS: [&str; 2] = ["Hark", "Levenshtein"];
/// `None` omits the field. The adapter sends `xhigh` (D7: `medium` and up
/// matched batch accuracy in CP0).
const DELAYS: [Option<&str>; 6] = [
    None,
    Some("minimal"),
    Some("low"),
    Some("medium"),
    Some("high"),
    Some("xhigh"),
];
const SETUP_TIMEOUT: Duration = Duration::from_secs(5);
const FINAL_TIMEOUT: Duration = Duration::from_secs(15);

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

struct Probe {
    key: String,
    realtime: bool,
    verbose: bool,
    /// Append without waiting for `session.updated` (D10).
    no_wait: bool,
}

/// What one turn did, for the summary table.
struct Turn {
    label: String,
    text_frames: usize,
    binary_frames: usize,
    deltas_before_commit: usize,
    deltas_after_commit: usize,
    first_delta_ms: Option<u128>,
    final_ms: Option<u128>,
    transcript: Option<String>,
    usage: Option<String>,
    failure: Option<String>,
}

impl Turn {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            text_frames: 0,
            binary_frames: 0,
            deltas_before_commit: 0,
            deltas_after_commit: 0,
            first_delta_ms: None,
            final_ms: None,
            transcript: None,
            usage: None,
            failure: None,
        }
    }
}

enum Next {
    Json(&'static str, Value, String),
    Closed(String),
    Ended,
    Timeout,
    Failed(String),
    Other(String),
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let Some(key) = env("OPENAI_API_KEY") else {
        eprintln!("set OPENAI_API_KEY to run this probe");
        std::process::exit(2);
    };
    let probe = Probe {
        key,
        realtime: env("PROBE_BURST").is_none(),
        verbose: env("PROBE_VERBOSE").is_some(),
        no_wait: env("PROBE_NO_WAIT").is_some(),
    };
    let custom_wav = env("PROBE_WAV");
    let fixture = custom_wav
        .clone()
        .unwrap_or_else(|| format!("{}/fixtures/spike_clip.wav", env!("CARGO_MANIFEST_DIR")));
    let wav_bytes = std::fs::read(&fixture).unwrap_or_else(|e| panic!("{fixture}: {e}"));
    let info = wav::parse_wav_16k_mono(&wav_bytes).expect("clip must be 16 kHz mono PCM16");
    let expected = match custom_wav {
        Some(_) => None,
        None => std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/expected.txt"
        ))
        .ok(),
    };
    // The adapter's own replay upsample, so the probe sends what it sends.
    let clip_24k = upsample_16k_to_24k(&info.samples).expect("16 kHz -> 24 kHz");

    println!("model: {MODEL}  clip: {:.1} s", info.duration_secs());
    println!(
        "pacing: {}",
        if probe.realtime {
            "real time (100 ms per append)"
        } else {
            "burst"
        }
    );
    println!(
        "setup: {}",
        if probe.no_wait {
            "append straight after session.update (PROBE_NO_WAIT, the adapter's D10)"
        } else {
            "wait for session.updated before appending"
        }
    );

    // Built by hand rather than with #[tokio::main], which needs tokio's
    // `macros` feature; the same current_thread runtime the adapter will own.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let turns = runtime.block_on(async {
        let mut turns = Vec::new();
        for (i, delay) in DELAYS.iter().enumerate() {
            let label = format!("delay={}", delay.unwrap_or("(omitted)"));
            let verbose = probe.verbose || i == 0;
            turns.push(
                run_turn(
                    &probe,
                    label,
                    INPUT_RATE,
                    *delay,
                    &clip_24k,
                    probe.realtime,
                    verbose,
                )
                .await,
            );
        }
        // Push-to-talk can release almost at once; find where commit refuses.
        for ms in [0usize, 50, 100, 200] {
            let samples = &clip_24k[..(INPUT_RATE as usize * ms / 1000).min(clip_24k.len())];
            turns.push(
                run_turn(
                    &probe,
                    format!("commit after {ms} ms"),
                    INPUT_RATE,
                    None,
                    samples,
                    false,
                    probe.verbose,
                )
                .await,
            );
        }
        // Hark's pipeline rate, to see how the server words the refusal.
        turns.push(
            run_turn(
                &probe,
                "rate=16000",
                16_000,
                None,
                &info.samples,
                false,
                true,
            )
            .await,
        );
        bad_key().await;
        if let Some(secs) = env("PROBE_IDLE_SECS").and_then(|v| v.parse::<u64>().ok()) {
            idle(&probe, secs).await;
        }
        turns
    });
    // Outside the runtime: the batch adapter is blocking reqwest, which must
    // never run on a tokio thread (LL-G Rust HIGH, blocking-io-on-tokio).
    drop(runtime);
    let baseline = batch_baseline(&probe.key, &wav_bytes);
    // Also outside the runtime: the adapter owns one and drives it with
    // block_on, which cannot nest inside another runtime's context.
    let adapter = [
        through_the_adapter(&probe.key, &wav_bytes),
        through_a_live_session(&probe.key, &clip_24k),
    ];
    summary(&turns, baseline, &adapter, expected.as_deref());
}

fn adapter_config(key: &str) -> ProviderConfig {
    ProviderConfig {
        kind: ProviderKind::OpenAiLive,
        label: "openai".into(),
        base_url: BASE_URL.into(),
        model: MODEL.into(),
        api_key: key.to_string(),
        bias_terms: BIAS_TERMS.iter().map(|t| t.to_string()).collect(),
        cleanup_instruction: None,
        live_mode: TranscribeMode::Verbatim,
    }
}

/// The fixture replayed through `hark_stt::build`: what Test connection and
/// the fallback after a dropped stream do. Returns the summary line.
fn through_the_adapter(key: &str, wav_bytes: &[u8]) -> String {
    println!("\n=== through the real adapter (replay) ===");
    let client = hark_stt::shared_client().expect("client");
    let adapter = match hark_stt::build(&adapter_config(key), client) {
        Ok(adapter) => adapter,
        Err(e) => {
            return format!(
                "adapter replay: did not build: {}",
                scrub(&e.to_string(), key)
            )
        }
    };
    let started = Instant::now();
    let line = match adapter.transcribe(wav_bytes) {
        Ok(t) => format!(
            "adapter replay: {} ms end to end, \"{}\"",
            started.elapsed().as_millis(),
            clip(&t.text)
        ),
        Err(e) => format!(
            "adapter replay: FAILED after {} ms: {}",
            started.elapsed().as_millis(),
            scrub(&e.to_string(), key)
        ),
    };
    println!("{line}");
    line
}

/// The clip streamed through `hark_stt::build_live` at real time, in 20 ms
/// pushes like the pump's, so the adapter's own 100 ms batching is exercised.
/// The number that matters is release to transcript.
fn through_a_live_session(key: &str, samples_24k: &[f32]) -> String {
    println!("\n=== through a live session, at real time ===");
    let Some(live) = hark_stt::build_live(&adapter_config(key)) else {
        let line = "adapter live: no live session in this build".to_string();
        println!("{line}");
        return line;
    };
    assert_eq!(
        live.input_rate(),
        INPUT_RATE,
        "the adapter must declare 24 kHz"
    );
    let opened = Instant::now();
    let mut session = match live.start_session() {
        Ok(s) => s,
        Err(e) => {
            return format!(
                "adapter live: session did not open: {}",
                scrub(&e.to_string(), key)
            )
        }
    };
    let connect_ms = opened.elapsed().as_millis();
    println!("session open in {connect_ms} ms");
    let push = INPUT_RATE as usize * PUMP_MS as usize / 1000;
    for chunk in samples_24k.chunks(push) {
        if let Err(e) = session.push(chunk) {
            return format!("adapter live: push failed: {}", scrub(&e.to_string(), key));
        }
        std::thread::sleep(Duration::from_millis(PUMP_MS));
    }
    let released = Instant::now();
    let line = match session.finish() {
        Ok(t) => format!(
            "adapter live: connect {connect_ms} ms, release to transcript {} ms, \"{}\"",
            released.elapsed().as_millis(),
            clip(&t.text)
        ),
        Err(e) => format!(
            "adapter live: FAILED {} ms after release: {}",
            released.elapsed().as_millis(),
            scrub(&e.to_string(), key)
        ),
    };
    println!("{line}");
    line
}

async fn run_turn(
    probe: &Probe,
    label: impl Into<String>,
    rate: u32,
    delay: Option<&str>,
    samples: &[f32],
    realtime: bool,
    verbose: bool,
) -> Turn {
    let mut turn = Turn::new(label);
    println!("\n=== {} ===", turn.label);
    let t0 = Instant::now();
    let mut socket = match connect(&probe.key).await {
        Ok(s) => s,
        Err(e) => {
            println!("{} connect failed: {e}", at(&t0));
            turn.failure = Some(format!("connect: {e}"));
            return turn;
        }
    };
    println!("{} connected", at(&t0));

    // session.created arrives unprompted; the update is acknowledged by
    // session.updated. Unless PROBE_NO_WAIT is set, nothing is appended until
    // the update is accepted; with it, the ack is read during the appends.
    let update = session_update(rate, delay);
    if verbose {
        println!("{} -> {update}", at(&t0));
    }
    if let Err(e) = socket.send(Message::Text(update.to_string().into())).await {
        turn.failure = Some(format!("send session.update: {e}"));
        return turn;
    }
    if probe.no_wait {
        println!("{} not waiting for session.updated", at(&t0));
    } else {
        loop {
            match next(&mut socket, SETUP_TIMEOUT).await {
                Next::Json(kind, value, raw) => {
                    let event = observe(
                        &mut turn, kind, &value, &raw, None, &None, &t0, verbose, probe,
                    );
                    if event == "session.updated" {
                        break;
                    }
                    if event == "error" {
                        return close(socket, turn).await;
                    }
                }
                other => {
                    end_with(&mut turn, other, "waiting for session.updated", &t0);
                    return close(socket, turn).await;
                }
            }
        }
    }

    let chunk = rate as usize * CHUNK_MS / 1000;
    let mut appended = 0;
    for piece in samples.chunks(chunk) {
        let append = append_message(&samples_to_pcm16_le(piece));
        if let Err(e) = socket.send(Message::Text(append.to_string().into())).await {
            turn.failure = Some(format!("append: {e}"));
            return turn;
        }
        appended += 1;
        if realtime {
            tokio::time::sleep(Duration::from_millis(CHUNK_MS as u64)).await;
        }
        // Keep reading while writing, or the server's output backs up.
        loop {
            match next(&mut socket, Duration::ZERO).await {
                Next::Json(kind, value, raw) => {
                    if observe(
                        &mut turn, kind, &value, &raw, None, &None, &t0, verbose, probe,
                    ) == "error"
                    {
                        return close(socket, turn).await;
                    }
                }
                Next::Timeout => break,
                other => {
                    end_with(&mut turn, other, "during append", &t0);
                    return close(socket, turn).await;
                }
            }
        }
    }
    println!("{} -> {appended} appends", at(&t0));

    let commit = commit_message();
    if let Err(e) = socket.send(Message::Text(commit.to_string().into())).await {
        turn.failure = Some(format!("commit: {e}"));
        return turn;
    }
    let committed_at = Instant::now();
    println!("{} -> input_audio_buffer.commit", at(&t0));

    let mut item_id: Option<String> = None;
    loop {
        match next(&mut socket, FINAL_TIMEOUT).await {
            Next::Json(kind, value, raw) => {
                let event = observe(
                    &mut turn,
                    kind,
                    &value,
                    &raw,
                    Some(committed_at),
                    &item_id,
                    &t0,
                    verbose,
                    probe,
                );
                match event.as_str() {
                    "input_audio_buffer.committed" => {
                        item_id = value["item_id"].as_str().map(str::to_owned);
                    }
                    "error" | "conversation.item.input_audio_transcription.failed" => break,
                    "conversation.item.input_audio_transcription.completed"
                        if turn.transcript.is_some() =>
                    {
                        break
                    }
                    _ => {}
                }
            }
            other => {
                end_with(&mut turn, other, "after commit", &t0);
                break;
            }
        }
    }
    close(socket, turn).await
}

/// Record one server event on the turn and return its `type`.
#[allow(clippy::too_many_arguments)]
fn observe(
    turn: &mut Turn,
    kind: &'static str,
    value: &Value,
    raw: &str,
    committed_at: Option<Instant>,
    item_id: &Option<String>,
    t0: &Instant,
    verbose: bool,
    probe: &Probe,
) -> String {
    match kind {
        "binary" => turn.binary_frames += 1,
        _ => turn.text_frames += 1,
    }
    let event = value["type"].as_str().unwrap_or("<no type>").to_string();
    if verbose {
        println!("{} <- [{kind}] {}", at(t0), clip(&scrub(raw, &probe.key)));
    }
    match event.as_str() {
        "conversation.item.input_audio_transcription.delta" => match committed_at {
            None => turn.deltas_before_commit += 1,
            Some(c) => {
                turn.deltas_after_commit += 1;
                turn.first_delta_ms.get_or_insert(c.elapsed().as_millis());
            }
        },
        "conversation.item.input_audio_transcription.completed" => {
            // Completions can arrive out of order across turns; only the one
            // for our committed item counts. Before `committed` arrives there
            // is nothing to match, so take it and say so.
            let matches = match item_id {
                Some(id) => value["item_id"].as_str() == Some(id),
                None => {
                    println!("{} !! completed before committed", at(t0));
                    true
                }
            };
            if matches {
                turn.transcript = value["transcript"].as_str().map(str::to_owned);
                turn.usage = Some(value["usage"].to_string());
                turn.final_ms = committed_at.map(|c| c.elapsed().as_millis());
            }
        }
        "conversation.item.input_audio_transcription.failed" => {
            turn.failure = Some(scrub(&value["error"].to_string(), &probe.key));
        }
        "error" => {
            let message = scrub(&value["error"].to_string(), &probe.key);
            println!("{} <- error {message}", at(t0));
            turn.failure = Some(message);
        }
        _ => {}
    }
    event
}

fn end_with(turn: &mut Turn, next: Next, phase: &str, t0: &Instant) {
    let what = match next {
        Next::Closed(frame) => format!("socket closed {phase}: {frame}"),
        Next::Ended => format!("stream ended {phase}"),
        Next::Timeout => format!("timed out {phase}"),
        Next::Failed(e) => format!("read error {phase}: {e}"),
        Next::Other(o) => format!("unexpected frame {phase}: {o}"),
        Next::Json(..) => unreachable!("handled by the caller"),
    };
    println!("{} !! {what}", at(t0));
    turn.failure.get_or_insert(what);
}

async fn close(mut socket: Socket, turn: Turn) -> Turn {
    let _ = socket.close(None).await;
    turn
}

async fn connect(key: &str) -> Result<Socket, String> {
    let url = realtime_url(BASE_URL).map_err(|e| e.to_string())?;
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    let mut auth = HeaderValue::from_str(&format!("Bearer {key}"))
        .map_err(|_| "the key is not a valid header value".to_string())?;
    auth.set_sensitive(true);
    request.headers_mut().insert("Authorization", auth);
    match tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(request),
    )
    .await
    {
        Err(_) => Err("no handshake within 10 s".into()),
        // A refused upgrade carries the HTTP status and OpenAI's JSON error.
        Ok(Err(WsError::Http(response))) => {
            let body = response
                .body()
                .as_deref()
                .map(String::from_utf8_lossy)
                .unwrap_or_default();
            Err(format!("HTTP {}: {}", response.status(), scrub(&body, key)))
        }
        Ok(Err(e)) => Err(scrub(&e.to_string(), key)),
        Ok(Ok((socket, _))) => Ok(socket),
    }
}

async fn next(socket: &mut Socket, wait: Duration) -> Next {
    match tokio::time::timeout(wait, socket.next()).await {
        Err(_) => Next::Timeout,
        Ok(None) => Next::Ended,
        Ok(Some(Err(e))) => Next::Failed(e.to_string()),
        Ok(Some(Ok(Message::Close(frame)))) => Next::Closed(format!("{frame:?}")),
        // JSON may arrive in either frame type (LL-G); report which one.
        Ok(Some(Ok(Message::Text(t)))) => json_frame("text", t.to_string()),
        Ok(Some(Ok(Message::Binary(b)))) => {
            json_frame("binary", String::from_utf8_lossy(&b).into_owned())
        }
        Ok(Some(Ok(other))) => Next::Other(format!("{other:?}")),
    }
}

fn json_frame(kind: &'static str, body: String) -> Next {
    match serde_json::from_str::<Value>(&body) {
        Ok(value) => Next::Json(kind, value, body),
        Err(_) => Next::Other(format!(
            "{kind} frame that is not JSON, {} bytes",
            body.len()
        )),
    }
}

/// The adapter's own `session.update`, with the rate and `delay` the probe is
/// varying swapped in. Everything else is exactly what the app sends.
fn session_update(rate: u32, delay: Option<&str>) -> Value {
    let terms: Vec<String> = BIAS_TERMS.iter().map(|t| t.to_string()).collect();
    let mut update = hark_stt::openai_live::session_update(MODEL, &terms);
    let input = &mut update["session"]["audio"]["input"];
    input["format"]["rate"] = json!(rate);
    let transcription = input["transcription"]
        .as_object_mut()
        .expect("the adapter's update has a transcription object");
    match delay {
        Some(delay) => {
            transcription.insert("delay".into(), json!(delay));
        }
        None => {
            transcription.remove("delay");
        }
    }
    update
}

/// A key that cannot be valid, to see how a refusal is shaped.
async fn bad_key() {
    println!("\n=== bad key ===");
    match connect("sk-hark-probe-not-a-real-key").await {
        Err(e) => println!("handshake refused: {e}"),
        Ok(mut socket) => {
            println!("handshake accepted; the refusal comes over the socket:");
            let update = session_update(INPUT_RATE, None);
            let _ = socket.send(Message::Text(update.to_string().into())).await;
            for _ in 0..3 {
                match next(&mut socket, SETUP_TIMEOUT).await {
                    Next::Json(kind, _, raw) => println!("<- [{kind}] {}", clip(&scrub(&raw, ""))),
                    Next::Closed(frame) => {
                        println!("<- CLOSE {frame}");
                        break;
                    }
                    _ => break,
                }
            }
            let _ = socket.close(None).await;
        }
    }
}

/// One configured socket held open with no audio. Check the usage page
/// afterwards: if this minute was billed, opening on key-down costs money.
async fn idle(probe: &Probe, secs: u64) {
    println!("\n=== idle socket for {secs} s ===");
    let t0 = Instant::now();
    let mut socket = match connect(&probe.key).await {
        Ok(s) => s,
        Err(e) => return println!("connect failed: {e}"),
    };
    let update = session_update(INPUT_RATE, None);
    let _ = socket.send(Message::Text(update.to_string().into())).await;
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        match next(
            &mut socket,
            deadline.saturating_duration_since(Instant::now()),
        )
        .await
        {
            Next::Json(kind, _, raw) => {
                println!("{} <- [{kind}] {}", at(&t0), clip(&scrub(&raw, &probe.key)))
            }
            Next::Timeout => break,
            Next::Closed(frame) => return println!("{} <- CLOSE {frame}", at(&t0)),
            _ => break,
        }
    }
    let _ = socket.close(None).await;
    println!("{} closed after idling", at(&t0));
}

/// gpt-transcribe through the real batch adapter: what OpenAI costs today. Run
/// twice and report the second, because the app's client is long-lived and
/// its TLS session is already warm by the time anyone dictates.
fn batch_baseline(key: &str, wav_bytes: &[u8]) -> Option<(u128, String)> {
    println!("\n=== gpt-transcribe batch (baseline) ===");
    let config = ProviderConfig {
        kind: ProviderKind::OpenAiTranscribe,
        label: "openai".into(),
        base_url: "https://api.openai.com/v1".into(),
        model: "gpt-transcribe".into(),
        api_key: key.to_string(),
        bias_terms: BIAS_TERMS.iter().map(|t| t.to_string()).collect(),
        cleanup_instruction: None,
        live_mode: TranscribeMode::Verbatim,
    };
    let client = hark_stt::shared_client().expect("client");
    let adapter = hark_stt::build(&config, client).expect("adapter builds");
    let mut warm = None;
    for attempt in ["cold", "warm"] {
        let started = Instant::now();
        match adapter.transcribe(wav_bytes) {
            Ok(t) => {
                let ms = started.elapsed().as_millis();
                println!("{attempt}: {ms} ms");
                warm = Some((ms, t.text));
            }
            Err(e) => {
                println!("{attempt}: FAILED {e}");
                warm = None;
            }
        }
    }
    warm
}

fn summary(
    turns: &[Turn],
    baseline: Option<(u128, String)>,
    adapter: &[String],
    expected: Option<&str>,
) {
    println!("\n=== summary (paste this back) ===");
    println!(
        "{:<20} {:>11} {:>14} {:>11} {:>9} {:>5}  result",
        "turn", "text/binary", "deltas pre/post", "1st delta", "final", "WER"
    );
    for t in turns {
        let wer = match (expected, &t.transcript) {
            (Some(e), Some(got)) => format!("{:.0}%", word_error_rate(e, got) * 100.0),
            _ => "-".into(),
        };
        let result = match (&t.failure, &t.transcript) {
            (Some(f), _) => format!("FAILED {}", clip(f)),
            (None, Some(text)) => format!("ok \"{}\"", clip(text)),
            (None, None) => "no transcript".into(),
        };
        println!(
            "{:<20} {:>11} {:>14} {:>11} {:>9} {:>5}  {result}",
            t.label,
            format!("{}/{}", t.text_frames, t.binary_frames),
            format!("{}/{}", t.deltas_before_commit, t.deltas_after_commit),
            ms(t.first_delta_ms),
            ms(t.final_ms),
            wer,
        );
    }
    if let Some(usage) = turns.iter().find_map(|t| t.usage.as_deref()) {
        println!("usage on a completed event: {usage}");
    }
    match baseline {
        Some((ms, text)) => {
            let wer = expected
                .map(|e| format!("{:.0}%", word_error_rate(e, &text) * 100.0))
                .unwrap_or_else(|| "-".into());
            println!(
                "gpt-transcribe batch, warm: {ms} ms for the whole clip after release, WER {wer}"
            );
        }
        None => println!("gpt-transcribe batch: failed"),
    }
    for line in adapter {
        println!("{line}");
    }
    println!("live 'final' is commit to completed: the time left after release.");
}

/// Word-level edit distance over the reference length, ignoring case and
/// punctuation. Enough to rank `delay` values against each other.
fn word_error_rate(expected: &str, got: &str) -> f64 {
    let words = |s: &str| -> Vec<String> {
        s.split_whitespace()
            .map(|w| {
                w.chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>()
                    .to_lowercase()
            })
            .filter(|w| !w.is_empty())
            .collect()
    };
    let (a, b) = (words(expected), words(got));
    if a.is_empty() {
        return 0.0;
    }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, wa) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, wb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if wa == wb {
                prev
            } else {
                1 + prev.min(row[j]).min(row[j + 1])
            };
            prev = cur;
        }
    }
    row[b.len()] as f64 / a.len() as f64
}

fn clip(text: &str) -> String {
    const MAX: usize = 300;
    match text.char_indices().nth(MAX) {
        Some((i, _)) => format!("{}...", &text[..i]),
        None => text.to_string(),
    }
}

fn at(t0: &Instant) -> String {
    format!("{:>6} ms", t0.elapsed().as_millis())
}

fn ms(value: Option<u128>) -> String {
    value.map_or_else(|| "-".into(), |v| format!("{v} ms"))
}
