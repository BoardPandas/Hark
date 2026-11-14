//! The socket side of an OpenAI live session: the authorised upgrade, appends
//! with the receive side drained, and the commit that finalises the turn.

use super::*;
use crate::live_transport::{frame_json, turn_driver};
use crate::wav::samples_to_pcm16_le;
use futures_util::{FutureExt, SinkExt, StreamExt};
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{header, HeaderValue, Request};
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

#[cfg(test)]
mod tests;

pub use crate::live_transport::Socket;

// WebSocket errors can carry frame contents, URLs or header values. Keep
// the category and the status, never their Display or Debug.
fn socket_error(operation: &str, error: &WsError) -> SttError {
    let category = match error {
        WsError::ConnectionClosed | WsError::AlreadyClosed => "connection closed",
        WsError::Io(error) => return fail(format!("{operation}: I/O {:?}", error.kind())),
        WsError::Tls(_) => "TLS failure",
        WsError::Capacity(_) | WsError::WriteBufferFull(_) => "capacity exceeded",
        WsError::Protocol(_) | WsError::AttackAttempt => "protocol error",
        WsError::Utf8(_) => "invalid UTF-8",
        WsError::Url(_) => "invalid URL",
        // A refused upgrade: the status, plus only the whitelisted reason
        // slugs `error_for_status` takes out of OpenAI's JSON body.
        WsError::Http(response) => {
            let body = response
                .body()
                .as_deref()
                .map(String::from_utf8_lossy)
                .unwrap_or_default();
            return crate::error::error_for_status(
                PROVIDER,
                response.status().as_u16(),
                crate::openai_compatible::retry_after_secs(response.headers()),
                &body,
            );
        }
        WsError::HttpFormat(_) => "invalid HTTP format",
    };
    fail(format!("{operation}: {category}"))
}

/// The upgrade request, with the key in a header marked sensitive and no
/// `OpenAI-Beta` header (that selects the pre-GA interface).
pub fn authorized_request(url: &str, api_key: &str) -> Result<Request<()>, SttError> {
    let mut request = url
        .into_client_request()
        .map_err(|e| socket_error("invalid socket URL", &e))?;
    let mut auth = HeaderValue::from_str(&format!("Bearer {api_key}"))
        .map_err(|_| fail("the API key is not a valid HTTP header value"))?;
    auth.set_sensitive(true);
    request.headers_mut().insert(header::AUTHORIZATION, auth);
    Ok(request)
}

/// Send one JSON message, bounded by [`SEND_TIMEOUT_MS`].
pub async fn send_json(socket: &mut Socket, value: Value, what: &str) -> Result<(), SttError> {
    tokio::time::timeout(
        Duration::from_millis(SEND_TIMEOUT_MS),
        socket.send(Message::Text(value.to_string().into())),
    )
    .await
    .map_err(|_| SttError::Timeout {
        provider: PROVIDER.to_string(),
        configured_ms: SEND_TIMEOUT_MS,
    })?
    .map_err(|e| socket_error(&format!("{what} send failed"), &e))
}

/// Open the socket and send `session.update`.
///
/// Does **not** wait for `session.updated` (D10). Client events on one
/// connection are processed in order, and CP0 measured the ack at about
/// 420 ms, which a short hold would otherwise pay at release. An `error`
/// that arrives instead fails the session from the next read, and the
/// pipeline replays the finished clip.
pub async fn connect(url: &str, api_key: &str, update: Value) -> Result<Socket, SttError> {
    let request = authorized_request(url, api_key)?;
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_millis(CONNECT_TIMEOUT_MS),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| SttError::Timeout {
        provider: PROVIDER.to_string(),
        configured_ms: CONNECT_TIMEOUT_MS,
    })?
    .map_err(|e| socket_error("websocket connect failed", &e))?;
    send_json(&mut socket, update, "session.update").await?;
    Ok(socket)
}

/// Feed one frame to the turn. `Ok(true)` once the transcript is in.
fn read_frame(frame: Message, turn: &mut Turn, phase: &str) -> Result<bool, SttError> {
    if let Some(body) = frame_json(&frame) {
        return turn.absorb(&body);
    }
    match frame {
        Message::Close(Some(frame)) => Err(close_error(
            u16::from(frame.code),
            frame.reason.as_str(),
            phase,
        )),
        Message::Close(None) => Err(fail(format!("the session closed {phase}"))),
        // tungstenite answers pings itself.
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => Ok(false),
        // `frame_json` declines only a binary frame that is not UTF-8.
        Message::Text(_) | Message::Binary(_) => {
            Err(fail("the server sent a binary frame that is not UTF-8"))
        }
    }
}

/// The most frames one drain takes, so a chatty server cannot turn a
/// drain into a stall.
const DRAIN_MAX_FRAMES: usize = 64;

/// Take whatever the server has already sent, without waiting for more.
///
/// Reading while writing keeps the receive side moving: deltas stream
/// throughout the hold (CP0, about 30 per turn), and a client that only
/// writes lets them back up. It is also where an `error` sent in answer to
/// `session.update` surfaces, so a refused session ends mid-hold rather
/// than at release.
///
/// # Why not `timeout(Duration::ZERO, socket.next())`
///
/// That was the first version, and on Windows it cost about 15 ms a call.
/// A zero deadline is still a timer: tokio rounds every deadline up to the
/// next 1 ms tick (`deadline_to_tick`), so whenever nothing is buffered —
/// the usual case — the runtime parks until that tick, and Windows' ~15.6
/// ms default timer resolution stretches the 1 ms to a whole scheduler
/// quantum. At one drain per 100 ms chunk a 10 s replay lost up to ~1.5 s,
/// and every release paid 50-80 ms. [`turn_driver`] plus `now_or_never`
/// never arms a timer at all; `an_empty_drain_never_waits_on_a_timer` and
/// `a_drain_sees_frames_that_landed_outside_the_runtime` pin both halves.
pub async fn drain(socket: &mut Socket, turn: &mut Turn) -> Result<(), SttError> {
    turn_driver().await;
    for _ in 0..DRAIN_MAX_FRAMES {
        let frame = match socket.next().now_or_never() {
            None => return Ok(()),
            Some(None) => return Err(fail("the session ended mid-stream")),
            Some(Some(Err(e))) => return Err(socket_error("socket read failed", &e)),
            Some(Some(Ok(frame))) => frame,
        };
        read_frame(frame, turn, "mid-stream")?;
    }
    Ok(())
}

/// The server's own account of a session a send just failed on, if it
/// gave one: an `error` event, a failed item, or a close frame.
///
/// D10 appends without waiting for `session.updated`, so a refused key's
/// `error` event and close 3000 race the appends. When the server hangs
/// up first, the next write fails with a bare I/O error or "connection
/// closed" while the real reason sits unread in the receive buffer, and a
/// bad key would read as a network fault. A read error or the end of the
/// stream says nothing the send error did not, so those leave it standing.
async fn server_reason(socket: &mut Socket, turn: &mut Turn) -> Option<SttError> {
    turn_driver().await;
    for _ in 0..DRAIN_MAX_FRAMES {
        let Some(Some(Ok(frame))) = socket.next().now_or_never() else {
            return None;
        };
        if let Err(reason) = read_frame(frame, turn, "mid-stream") {
            return Some(reason);
        }
    }
    None
}

/// `send_error`, unless the server has already said why the session
/// ended, in which case that (an `Auth 401` for a refused key, say).
pub async fn explain_send_failure(
    socket: &mut Socket,
    turn: &mut Turn,
    send_error: SttError,
) -> SttError {
    server_reason(socket, turn).await.unwrap_or(send_error)
}

/// [`send_json`], with a failure explained by the server where it can be.
async fn send_or_explain(
    socket: &mut Socket,
    turn: &mut Turn,
    value: Value,
    what: &str,
) -> Result<(), SttError> {
    match send_json(socket, value, what).await {
        Ok(()) => Ok(()),
        Err(error) => Err(explain_send_failure(socket, turn, error).await),
    }
}

/// Send samples at [`INPUT_RATE`] as 100 ms appends, without reading.
pub async fn send_audio(
    socket: &mut Socket,
    samples: &[f32],
    turn: &mut Turn,
) -> Result<(), SttError> {
    for chunk in samples.chunks(CHUNK_SAMPLES) {
        let message = append_message(&samples_to_pcm16_le(chunk));
        send_or_explain(socket, turn, message, "append").await?;
    }
    Ok(())
}

/// [`send_audio`], then one [`drain`]: once per call, not once per chunk.
/// The drain runs even when there was nothing to send, so an `error`
/// ends the session while the fallback still has time.
pub async fn append(socket: &mut Socket, samples: &[f32], turn: &mut Turn) -> Result<(), SttError> {
    send_audio(socket, samples, turn).await?;
    drain(socket, turn).await
}

/// Commit the turn and read until its transcript arrives.
///
/// The caller has already appended at least [`MIN_COMMIT_SAMPLES`].
pub async fn finish(
    socket: &mut Socket,
    turn: &mut Turn,
    budget: Finalize,
) -> Result<String, SttError> {
    send_or_explain(socket, turn, commit_message(), "commit").await?;
    // Marked after the send: nothing is read in between, so a
    // `committed` absorbed from here on was read once the server had our
    // commit, or was already on the wire. The pump drains every 20 ms
    // and the replay after every chunk, so that window is a frame or two
    // wide, and only a server-side commit could land in it.
    turn.sent_commit();
    let started = Instant::now();
    let total = Duration::from_millis(budget.total_ms);
    let per_frame = Duration::from_millis(budget.per_frame_ms);
    let stalled = |turn: &Turn, waited_ms: u64| {
        // A bare timeout cannot tell "the commit never landed" from "still
        // transcribing"; the counts can. Counts only, never text.
        log::warn!(
            "openai live finalise stalled after {waited_ms} ms: {} frames, {} deltas, \
             committed: {}, configured: {}",
            turn.frames(),
            turn.deltas(),
            turn.committed(),
            turn.configured()
        );
        SttError::Stalled {
            provider: PROVIDER.to_string(),
            waited_ms,
        }
    };
    let text = loop {
        if let Some(text) = turn.take_transcript() {
            break text;
        }
        let remaining = match total.checked_sub(started.elapsed()) {
            Some(left) if !left.is_zero() => left.min(per_frame),
            _ => return Err(stalled(turn, budget.total_ms)),
        };
        let Ok(frame) = tokio::time::timeout(remaining, socket.next()).await else {
            let waited = if remaining < per_frame {
                budget.total_ms
            } else {
                budget.per_frame_ms
            };
            return Err(stalled(turn, waited));
        };
        let Some(frame) = frame else {
            return Err(fail(format!(
                "the session ended before the transcript arrived ({} frames, committed: {})",
                turn.frames(),
                turn.committed()
            )));
        };
        let frame = frame.map_err(|e| socket_error("socket read failed", &e))?;
        read_frame(frame, turn, "before the transcript arrived")?;
    };
    if !turn.configured() {
        // In order on one connection, so the ack precedes the commit's
        // answer. Its absence is worth knowing about, not worth failing.
        log::warn!("openai live: transcript arrived without session.updated");
    }
    // Bounded like every other write: a close to a peer that has stopped
    // reading must not outlive the dictation it belongs to.
    let _ = tokio::time::timeout(Duration::from_millis(SEND_TIMEOUT_MS), socket.close(None)).await;
    Ok(text)
}

/// One complete session from a finished clip at [`INPUT_RATE`].
pub async fn run(
    url: &str,
    api_key: &str,
    update: Value,
    mut samples: Vec<f32>,
    budget: Finalize,
) -> Result<String, SttError> {
    let mut socket = connect(url, api_key, update).await?;
    let mut turn = Turn::default();
    samples.resize(samples.len() + commit_padding(samples.len()), 0.0);
    // A replay is a burst, not a hold, so it drains after every chunk: a
    // write-only burst can back the server's output up (LL-G Rust HIGH,
    // `websocket-binary-frames-not-text`). A drain is one timer-free
    // driver turn, so this costs microseconds per chunk.
    for chunk in samples.chunks(CHUNK_SAMPLES) {
        append(&mut socket, chunk, &mut turn).await?;
    }
    finish(&mut socket, &mut turn, budget).await
}
