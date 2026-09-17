//! Dump every frame a Gemini Live session sends, for diagnosing the wire
//! protocol against the real API.
//!
//! The adapter classifies frames and throws away what it does not recognise,
//! which is correct in production and useless when the question is "what is
//! the server actually saying?". This prints frames raw, in order, with
//! timings, so a protocol mistake is visible rather than inferred from a
//! timeout.
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run -p hark-stt --features live \
//!     --example gemini_live_probe
//! ```
//!
//! Knobs: `GEMINI_LIVE_MODEL` (default gemini-3.5-transcribe-live),
//! `GEMINI_LIVE_MODE` (verbatim | smart, default verbatim),
//! `PROBE_NO_WAIT=1` to skip waiting for `setupComplete` — which reproduces
//! the bug this probe was written to confirm.
//!
//! The API key rides in the socket URL, so nothing here prints a URL.

use futures_util::{SinkExt, StreamExt};
use hark_stt::gemini_live::{
    activity_end_message, activity_start_message, audio_message, audio_stream_end_message,
    live_url, samples_to_pcm16_le, setup_message, TranscribeMode, CHUNK_SAMPLES,
};
use hark_stt::wav;
use hark_stt::{ProviderConfig, ProviderKind};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// The Live API sends its JSON in BINARY frames, not text ones. A probe that
/// only printed `Message::Text` showed an empty conversation.
fn body_of(frame: &Message) -> Option<String> {
    match frame {
        Message::Text(t) => Some(t.to_string()),
        Message::Binary(b) => Some(String::from_utf8_lossy(b).replace('\n', "")),
        _ => None,
    }
}

fn main() {
    // Show the adapter's own log lines; the diagnostics it emits on a failed
    // finalise are the whole point of running this.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    // Built by hand rather than with #[tokio::main]: that macro needs tokio's
    // `macros` feature, and widening a shipped dependency for one example is
    // the wrong trade. This is the same current_thread runtime the adapter
    // builds for itself.
    let (model, mode, key, wav_bytes) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(probe());
    // Outside the runtime context now, so the adapter can build its own.
    through_the_adapter(&model, mode, &key, &wav_bytes);
}

async fn probe() -> (String, TranscribeMode, String, Vec<u8>) {
    let Some(key) = env("GEMINI_API_KEY") else {
        eprintln!("set GEMINI_API_KEY to run this probe");
        std::process::exit(2);
    };
    let model = env("GEMINI_LIVE_MODEL").unwrap_or_else(|| "gemini-3.5-transcribe-live".into());
    let mode = match env("GEMINI_LIVE_MODE").as_deref() {
        Some("smart") => TranscribeMode::Smart,
        _ => TranscribeMode::Verbatim,
    };
    let wait_for_ack = env("PROBE_NO_WAIT").is_none();

    let fixture = format!("{}/fixtures/spike_clip.wav", env!("CARGO_MANIFEST_DIR"));
    let wav_bytes = std::fs::read(&fixture).expect("fixtures/spike_clip.wav must exist");
    let info = wav::parse_wav_16k_mono(&wav_bytes).expect("fixture must be 16 kHz mono PCM16");
    // Scale the fixture to imitate a quiet microphone: the streaming path
    // sends audio at capture level, and a too-quiet signal may never trip the
    // server's activity detection.
    let gain: f32 = env("PROBE_GAIN")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    let scaled: Vec<f32> = info.samples.iter().map(|s| s * gain).collect();
    let pcm = samples_to_pcm16_le(&scaled);

    println!("model: {model}  mode: {}", mode.wire());
    println!(
        "fixture: {:.1} s, {} PCM bytes, {} frames of 100 ms",
        info.duration_secs(),
        pcm.len(),
        pcm.len().div_ceil(CHUNK_SAMPLES * 2)
    );
    println!("waiting for setupComplete before sending audio: {wait_for_ack}");
    println!("---");

    let t0 = Instant::now();
    let at = |t: &Instant| format!("{:>6} ms", t.elapsed().as_millis());

    let (mut socket, _) = match tokio_tungstenite::connect_async(live_url(&key)).await {
        Ok(ok) => ok,
        // Never print the error verbatim: tungstenite echoes the URL, which
        // carries the key.
        Err(e) => {
            eprintln!(
                "connect failed: {}",
                hark_stt::gemini_live::scrub(&e.to_string())
            );
            std::process::exit(1);
        }
    };
    println!("{} connected", at(&t0));

    let setup = setup_message(
        &model,
        &["Hark".to_string(), "Levenshtein".to_string()],
        mode,
    );
    println!("{} -> setup {}", at(&t0), setup);
    socket
        .send(Message::Text(setup.to_string().into()))
        .await
        .expect("setup send");

    if wait_for_ack {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), socket.next()).await {
                Ok(Some(Ok(ref f))) if body_of(f).is_some() => {
                    let body = body_of(f).expect("checked");
                    println!("{} <- {body}", at(&t0));
                    if body.contains("setupComplete") {
                        break;
                    }
                }
                Ok(Some(Ok(Message::Close(f)))) => {
                    println!("{} <- CLOSE {f:?}", at(&t0));
                    return (model, mode, key, wav_bytes);
                }
                Ok(Some(Ok(other))) => println!("{} <- {other:?}", at(&t0)),
                Ok(Some(Err(e))) => {
                    println!("{} <- read error: {e}", at(&t0));
                    return (model, mode, key, wav_bytes);
                }
                Ok(None) => {
                    println!("{} <- stream ended before setupComplete", at(&t0));
                    return (model, mode, key, wav_bytes);
                }
                Err(_) => {
                    println!("{} !! no setupComplete within 5 s", at(&t0));
                    break;
                }
            }
        }
    }

    // Activity detection is disabled in setup, so the turn must be opened
    // explicitly or the server never starts one.
    socket
        .send(Message::Text(activity_start_message().to_string().into()))
        .await
        .expect("activityStart send");
    println!("{} -> activityStart", at(&t0));

    let mut sent = 0;
    for chunk in pcm.chunks(CHUNK_SAMPLES * 2) {
        socket
            .send(Message::Text(audio_message(chunk).to_string().into()))
            .await
            .expect("audio send");
        sent += 1;
        // Drain whatever arrived without waiting, so interims appear in order.
        while let Ok(Some(Ok(frame))) = tokio::time::timeout(Duration::ZERO, socket.next()).await {
            if let Some(body) = body_of(&frame) {
                println!("{} <- {body}", at(&t0));
            }
        }
    }
    println!("{} -> sent {sent} audio frames", at(&t0));

    socket
        .send(Message::Text(activity_end_message().to_string().into()))
        .await
        .expect("activityEnd send");
    socket
        .send(Message::Text(audio_stream_end_message().to_string().into()))
        .await
        .expect("audioStreamEnd send");
    println!("{} -> activityEnd + audioStreamEnd", at(&t0));

    loop {
        match tokio::time::timeout(Duration::from_secs(10), socket.next()).await {
            Ok(Some(Ok(ref f))) if body_of(f).is_some() => {
                let body = body_of(f).expect("checked");
                println!("{} <- {body}", at(&t0));
                // Stop where the adapter stops, so the probe measures the same
                // thing the product experiences.
                if body.contains("generationComplete") || body.contains("turnComplete") {
                    println!("{} == turn complete", at(&t0));
                    break;
                }
            }
            Ok(Some(Ok(Message::Close(f)))) => {
                println!("{} <- CLOSE {f:?}", at(&t0));
                break;
            }
            Ok(Some(Ok(other))) => println!("{} <- {other:?}", at(&t0)),
            Ok(Some(Err(e))) => {
                println!("{} <- read error: {e}", at(&t0));
                break;
            }
            Ok(None) => {
                println!("{} <- stream ended", at(&t0));
                break;
            }
            Err(_) => {
                println!("{} !! nothing further within 10 s", at(&t0));
                break;
            }
        }
    }
    let _ = socket.close(None).await;
    println!("--- raw protocol done in {} ms", t0.elapsed().as_millis());
    (model, mode, key, wav_bytes)
}

/// The same clip through the real adapter, which is what actually has to work.
/// A probe that only proves the wire format leaves the product untested.
///
/// Called from `main`, NOT from inside `probe()`: `transcribe` owns its own
/// runtime and drives it with `block_on`, which cannot nest inside another
/// runtime's context.
fn through_the_adapter(model: &str, mode: TranscribeMode, key: &str, wav_bytes: &[u8]) {
    println!("\n=== through the real adapter ===");
    let config = ProviderConfig {
        kind: ProviderKind::GeminiLive,
        label: "gemini".into(),
        base_url: String::new(),
        model: model.to_string(),
        api_key: key.to_string(),
        bias_terms: vec!["Hark".into(), "Levenshtein".into()],
        cleanup_instruction: None,
        live_mode: mode,
    };
    let client = hark_stt::shared_client().expect("client");
    let adapter = hark_stt::build(&config, client).expect("adapter builds");
    let started = Instant::now();
    match adapter.transcribe(wav_bytes) {
        Ok(t) => {
            println!(
                "transcript ({} ms): {}",
                started.elapsed().as_millis(),
                t.text
            );
            match t.cleaned {
                Some(c) => println!("cleaned: {c}"),
                None => println!("cleaned: none (verbatim mode)"),
            }
        }
        Err(e) => println!("FAILED after {} ms: {e}", started.elapsed().as_millis()),
    }
}
