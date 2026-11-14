//! The socket side of a Gemini Live session: connect and setup, sending audio
//! while keeping what the server says back, and finalising the turn.

use super::*;
use crate::live_transport::{frame_json, turn_driver};
use futures_util::{FutureExt, SinkExt, StreamExt};
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

#[cfg(test)]
mod privacy_tests;

pub use crate::live_transport::Socket;

// WebSocket errors can contain frame contents, URLs, or header values.
// Preserve the failure category and status, never their Display/Debug.
fn socket_error(operation: &str, error: &tokio_tungstenite::tungstenite::Error) -> SttError {
    use tokio_tungstenite::tungstenite::Error;
    let category = match error {
        Error::ConnectionClosed | Error::AlreadyClosed => "connection closed",
        Error::Io(error) => return fail(format!("{operation}: I/O {:?}", error.kind())),
        Error::Tls(_) => "TLS failure",
        Error::Capacity(_) | Error::WriteBufferFull(_) => "capacity exceeded",
        Error::Protocol(_) | Error::AttackAttempt => "protocol error",
        Error::Utf8(_) => "invalid UTF-8",
        Error::Url(_) => "invalid URL",
        Error::Http(response) => {
            return fail(format!("{operation}: HTTP {}", response.status().as_u16()))
        }
        Error::HttpFormat(_) => "invalid HTTP format",
    };
    fail(format!("{operation}: {category}"))
}

fn setup_closed(frame: Option<tokio_tungstenite::tungstenite::protocol::CloseFrame>) -> SttError {
    fail(match frame {
        Some(frame) => format!(
            "setup rejected (WebSocket close code {})",
            u16::from(frame.code)
        ),
        None => "the session closed during setup".to_string(),
    })
}

/// Send one JSON message, bounded by [`SEND_TIMEOUT_MS`].
///
/// Every write in this module goes through here. A bare `send().await` is
/// unbounded, and these run on the pipeline worker thread inside
/// `block_on` — see [`SEND_TIMEOUT_MS`].
pub async fn send_json(socket: &mut Socket, value: Value, what: &str) -> Result<(), SttError> {
    tokio::time::timeout(
        Duration::from_millis(SEND_TIMEOUT_MS),
        socket.send(Message::Text(value.to_string().into())),
    )
    .await
    .map_err(|_| SttError::Timeout {
        provider: "gemini-live".to_string(),
        configured_ms: SEND_TIMEOUT_MS,
    })?
    .map_err(|e| socket_error(&format!("{what} send failed"), &e))
}

/// Open the socket and send the opening `setup`. Everything after this is
/// incremental, so the caller can start pushing audio immediately.
pub async fn connect(url: String, setup: Value) -> Result<Socket, SttError> {
    let connect = tokio_tungstenite::connect_async(&url);
    let (mut socket, _) = tokio::time::timeout(Duration::from_millis(CONNECT_TIMEOUT_MS), connect)
        .await
        .map_err(|_| SttError::Timeout {
            provider: "gemini-live".to_string(),
            configured_ms: CONNECT_TIMEOUT_MS,
        })?
        .map_err(|e| socket_error("websocket connect failed", &e))?;
    send_json(&mut socket, setup, "setup").await?;

    // The Live API requires the client to wait for setupComplete before
    // sending anything else. Audio pushed ahead of the ack is discarded,
    // and the session then finalises an empty turn -- which surfaces as a
    // finalise timeout with no hint of the real cause.
    let deadline = Duration::from_millis(CONNECT_TIMEOUT_MS);
    loop {
        let frame = tokio::time::timeout(deadline, socket.next())
            .await
            .map_err(|_| SttError::Timeout {
                provider: "gemini-live".to_string(),
                configured_ms: CONNECT_TIMEOUT_MS,
            })?;
        let Some(frame) = frame else {
            return Err(fail(
                "the session closed before acknowledging setup".to_string(),
            ));
        };
        let frame = frame.map_err(|e| socket_error("socket read failed", &e))?;
        if let Some(body) = frame_json(&frame) {
            match parse_server_message(&body)? {
                ServerEvent::SetupComplete => break,
                // A transcript before the ack would be extraordinary, but
                // treating it as noise would lose it.
                ServerEvent::Final(text) => {
                    return Err(fail(format!(
                        "server sent a transcript of {} chars before acknowledging setup",
                        text.chars().count()
                    )))
                }
                _ => continue,
            }
        }
        match frame {
            Message::Close(frame) => {
                // The close frame is where an invalid setup actually
                // reports itself; without this the failure is a timeout
                // with no reason attached.
                return Err(setup_closed(frame));
            }
            _ => continue,
        }
    }
    // Automatic detection is off, so the turn must be opened by hand.
    send_json(&mut socket, activity_start_message(), "activityStart").await?;
    Ok(socket)
}

/// Push one batch of PCM as 100 ms frames, collecting anything the server
/// says back into `segments`.
pub async fn send_audio(
    socket: &mut Socket,
    pcm: &[u8],
    segments: &mut Vec<String>,
) -> Result<bool, SttError> {
    let mut complete = false;
    for chunk in pcm.chunks(CHUNK_SAMPLES * 2) {
        send_json(socket, audio_message(chunk), "audio").await?;
        // Keep the receive side moving while sending. Interims start
        // arriving immediately, and a client that only writes lets them
        // back up until the connection stops making progress.
        complete |= drain_into(socket, segments).await;
    }
    Ok(complete)
}

/// Take whatever the server has already sent, without waiting for more.
///
/// **This is where most of a dictation arrives.** `inputTranscription` is
/// finalised per pause and streams throughout the hold, so by the time the
/// key comes up the server has usually already sent most of the utterance.
/// An earlier version of this function read those frames and dropped them
/// to stop the receive buffer backing up, which left only whatever landed
/// after the turn was closed — the last few words — to be injected. Reading
/// and keeping costs nothing extra; reading and discarding was the bug.
///
/// Returns whether the turn completed while draining. Bounded at 64 frames
/// so a chatty server cannot turn a drain into a stall.
///
/// # Why not `timeout(Duration::ZERO, socket.next())`
///
/// That was the first version, and on Windows it cost about 15 ms a call.
/// A zero deadline is still a timer: tokio rounds it up to the next 1 ms
/// tick, so whenever nothing is buffered — the usual case — the runtime
/// parks until that tick, and Windows' ~15.6 ms default timer resolution
/// stretches the 1 ms to a whole scheduler quantum. This runs after every
/// 100 ms chunk, so a 25 s meeting chunk lost about 3.9 s and a 10 s
/// replay up to 1.5 s. [`turn_driver`] plus `now_or_never` never arms a
/// timer; `an_empty_drain_never_waits_on_a_timer` and
/// `a_drain_sees_frames_that_landed_outside_the_runtime` pin both halves.
pub async fn drain_into(socket: &mut Socket, segments: &mut Vec<String>) -> bool {
    turn_driver().await;
    for _ in 0..64 {
        let frame = match socket.next().now_or_never() {
            Some(Some(Ok(frame))) => frame,
            // Nothing buffered, stream ended, or a read error: all mean
            // "stop draining". A real error resurfaces in finish.
            _ => return false,
        };
        let Some(body) = frame_json(&frame) else {
            continue;
        };
        // A malformed frame is not worth failing a live dictation over
        // mid-hold; finish reads the same socket and will report it.
        if let Ok(ServerEvent::TurnComplete) = absorb(&body, segments) {
            return true;
        }
    }
    false
}

/// End the turn and read until the transcript is final.
///
/// `segments` carries whatever already arrived during the hold; this only
/// appends the tail. `budget` says how long the server may stay silent
/// ([`Finalize::STREAM`] for a streamed turn, [`Finalize::REPLAY`] for a
/// replay with the whole clip still to do) and whether a turn that never
/// produced a word is an empty transcript rather than a timeout.
pub async fn finish(
    socket: &mut Socket,
    mode: TranscribeMode,
    mut segments: Vec<String>,
    budget: Finalize,
) -> Result<String, SttError> {
    let per_frame_ms = budget.per_frame_ms;
    let total_ms = budget.total_ms;
    // Close the turn by hand. `activityEnd` is the signal that matters:
    // with automatic activity detection disabled, it is the only documented
    // way to tell the server speech has ended, and `audioStreamEnd` alone
    // is defined for the automatic-VAD case and leaves the turn open. This
    // pair is what was verified against the real API.
    send_json(socket, activity_end_message(), "activityEnd").await?;
    send_json(socket, audio_stream_end_message(), "audioStreamEnd").await?;

    let mut frames = 0u32;
    let mut interims = 0u32;
    let started = Instant::now();
    let total = Duration::from_millis(total_ms);
    let per_frame = Duration::from_millis(per_frame_ms);
    loop {
        // Whichever bound is closer. Without the total, every arriving
        // frame renews the per-frame budget and the loop never has to end.
        let remaining = match total.checked_sub(started.elapsed()) {
            Some(left) if !left.is_zero() => left.min(per_frame),
            _ => {
                if budget.quiet_is_empty && interims == 0 && segments.is_empty() {
                    return Ok(quiet(socket, frames).await);
                }
                log::warn!(
                    "gemini live finalise hit its {total_ms} ms ceiling: \
                     {frames} frames seen, {interims} interim, {} final",
                    segments.len()
                );
                return Err(SttError::Stalled {
                    provider: "gemini-live".to_string(),
                    waited_ms: total_ms,
                });
            }
        };
        let frame = tokio::time::timeout(remaining, socket.next()).await;
        if frame.is_err() && budget.quiet_is_empty && interims == 0 && segments.is_empty() {
            return Ok(quiet(socket, frames).await);
        }
        let frame = frame.map_err(|_| {
            // A bare timeout cannot distinguish "the server never
            // heard us" from "it is still thinking"; the counts can.
            // The mode is in the line because SMART has a failure of
            // its own with exactly this shape (see TranscribeMode).
            log::warn!(
                "gemini live finalise timed out after {per_frame_ms} ms ({} mode): \
                     {frames} frames seen, {interims} interim, {} final",
                mode.wire(),
                segments.len()
            );
            SttError::Stalled {
                provider: "gemini-live".to_string(),
                waited_ms: per_frame_ms,
            }
        })?;
        let Some(frame) = frame else {
            return Err(fail("the session ended before turn completion".into()));
        };
        let frame = frame.map_err(|e| socket_error("socket read failed", &e))?;
        frames += 1;
        if let Some(body) = frame_json(&frame) {
            if let Ok(v) = serde_json::from_str::<Value>(&body) {
                let inner = v.get("serverContent").map(top_keys).unwrap_or_default();
                log::debug!("gemini live frame {frames}: {:?} / {inner:?}", top_keys(&v));
            }
            match absorb(&body, &mut segments)? {
                ServerEvent::TurnComplete => break,
                ServerEvent::Interim => interims += 1,
                ServerEvent::Final(_) | ServerEvent::SetupComplete | ServerEvent::Other => {}
            }
            continue;
        }
        if let Message::Close(frame) = frame {
            // Even a normal socket close does not prove that the whole
            // recording was transcribed. Fail so the preserved clip can
            // replay; never expose the server's free-form close reason.
            return Err(fail(match frame {
                Some(frame) => format!(
                    "the session closed before turn completion (WebSocket close code {})",
                    u16::from(frame.code)
                ),
                None => "the session closed before turn completion".into(),
            }));
        }
    }
    // Bounded like every other write: a close handshake to a peer that has
    // stopped reading must not outlive the dictation it belongs to.
    let _ = tokio::time::timeout(Duration::from_millis(SEND_TIMEOUT_MS), socket.close(None)).await;

    if budget.quiet_is_empty && segments.is_empty() {
        return Ok(String::new());
    }
    collected(mode, &segments)
}

/// The server heard nothing it would transcribe: close the socket (bounded,
/// like every write) and report an empty transcript.
async fn quiet(socket: &mut Socket, frames: u32) -> String {
    log::debug!("gemini live: no speech in the clip ({frames} frames seen); empty transcript");
    let _ = tokio::time::timeout(Duration::from_millis(SEND_TIMEOUT_MS), socket.close(None)).await;
    String::new()
}

/// The finished transcript, or the error for a session that produced none.
pub fn collected(mode: TranscribeMode, segments: &[String]) -> Result<String, SttError> {
    let text = join_segments(segments);
    if text.is_empty() {
        return Err(fail(format!(
            "session ended with no finalised transcript ({} mode)",
            mode.wire()
        )));
    }
    Ok(text)
}

/// One complete session from a finished clip: the non-streaming path, used
/// when the pipeline could not stream during the hold.
pub async fn run(
    url: String,
    setup: Value,
    pcm: Vec<u8>,
    mode: TranscribeMode,
    budget: Finalize,
) -> Result<String, SttError> {
    let mut socket = connect(url, setup).await?;
    let mut segments = Vec::new();
    // Replayed far faster than real time, so the server usually says
    // nothing until the turn is closed -- but when it does speak early,
    // that text is the transcript and must not be dropped.
    if send_audio(&mut socket, &pcm, &mut segments).await? {
        return collected(mode, &segments);
    }
    finish(&mut socket, mode, segments, budget).await
}
