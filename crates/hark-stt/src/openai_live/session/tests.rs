//! The session against a local WebSocket peer: the finaliser, a whole
//! replay, and a streamed session driven the way the pipeline worker drives
//! it.

use super::*;
use base64::Engine;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::handshake::server::{Request as ServerRequest, Response};
use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame, Role};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const KEY: &str = "sk-proj-hark-test-SECRET";
const PRIVATE: &str = "PRIVATE_transcript_and_key";

fn rendered(error: &SttError) -> String {
    format!("{error} {error:?}")
}

fn assert_private(error: &SttError) {
    let text = rendered(error);
    for secret in [KEY, PRIVATE, "Bearer", "sk-"] {
        assert!(!text.contains(secret), "{secret} leaked: {text}");
    }
}

fn event(value: Value) -> Message {
    Message::Text(value.to_string().into())
}

fn committed(item_id: &str) -> Value {
    json!({"type": "input_audio_buffer.committed", "item_id": item_id, "previous_item_id": null})
}

fn completed(item_id: &str, transcript: &str) -> Value {
    json!({
        "type": "conversation.item.input_audio_transcription.completed",
        "item_id": item_id,
        "content_index": 0,
        "transcript": transcript,
        "usage": {"type": "duration", "seconds": 1}
    })
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Exercise the real finalizer against a local WebSocket peer. No TLS,
/// key or external service is involved. The peer reads the commit,
/// sends `frames`, then reads until the client goes away and returns
/// whether a close frame arrived.
fn finish_from_peer(frames: Vec<Message>, budget: Finalize) -> (Result<String, SttError>, bool) {
    finish_against(frames, budget, false)
}

/// As [`finish_from_peer`]; with `hang_up` the peer drops the TCP
/// connection straight after its frames instead.
fn finish_against(
    frames: Vec<Message>,
    budget: Finalize,
    hang_up: bool,
) -> (Result<String, SttError>, bool) {
    runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let peer = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
                let frame = socket.next().await.unwrap().unwrap();
                let actual: Value = serde_json::from_str(&frame_json(&frame).unwrap()).unwrap();
                assert_eq!(actual, commit_message());
                for frame in frames {
                    // The client may already have given up (a stall
                    // test); that is its answer, not a peer failure.
                    if socket.send(frame).await.is_err() {
                        return false;
                    }
                }
                if hang_up {
                    return false;
                }
                let mut closed = false;
                while let Some(Ok(frame)) = socket.next().await {
                    closed |= matches!(frame, Message::Close(_));
                }
                closed
            });
            let stream = tokio::net::TcpStream::connect(address).await.unwrap();
            let mut socket =
                WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None)
                    .await;
            let mut turn = Turn::default();
            let result = finish(&mut socket, &mut turn, budget).await;
            drop(socket);
            (result, peer.await.unwrap())
        })
        .await
        .expect("local WebSocket finalization must terminate")
    })
}

#[test]
fn the_transcript_for_the_committed_item_finishes_the_turn() {
    let (result, closed) = finish_from_peer(
        vec![
            event(json!({"type": "session.updated", "session": {}})),
            event(
                json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": "item_1", "delta": "hel"}),
            ),
            event(committed("item_1")),
            event(json!({"type": "conversation.item.added", "item": {}})),
            event(json!({"type": "conversation.item.done", "item": {}})),
            event(completed("item_1", "Hello there.")),
        ],
        Finalize::STREAM,
    );
    assert_eq!(result.unwrap(), "Hello there.");
    assert!(closed, "a finished session closes its socket");
}

#[test]
fn another_items_completion_is_ignored() {
    let (result, _) = finish_from_peer(
        vec![
            event(committed("item_ours")),
            event(completed("item_other", "not what was said")),
            event(completed("item_ours", "What was said.")),
        ],
        Finalize::STREAM,
    );
    assert_eq!(result.unwrap(), "What was said.");
}

#[test]
fn binary_frames_carry_the_protocol_too() {
    let binary = |value: Value| Message::Binary(value.to_string().into_bytes().into());
    let (result, _) = finish_from_peer(
        vec![
            binary(committed("item_1")),
            binary(completed("item_1", "Binary works.")),
        ],
        Finalize::STREAM,
    );
    assert_eq!(result.unwrap(), "Binary works.");
}

#[test]
fn a_close_before_the_transcript_fails_without_its_reason() {
    for code in [CloseCode::Normal, CloseCode::Error, CloseCode::Policy] {
        let (result, _) = finish_from_peer(
            vec![
                event(committed("item_1")),
                Message::Close(Some(CloseFrame {
                    code,
                    reason: PRIVATE.into(),
                })),
            ],
            Finalize::STREAM,
        );
        let error = result.unwrap_err();
        assert!(error.to_string().contains(&u16::from(code).to_string()));
        assert_private(&error);
    }
}

#[test]
fn close_code_3000_is_an_auth_refusal() {
    let (result, _) = finish_from_peer(
        vec![Message::Close(Some(CloseFrame {
            code: CloseCode::from(3000),
            reason: "invalid_request_error.invalid_api_key".into(),
        }))],
        Finalize::STREAM,
    );
    match result.unwrap_err() {
        SttError::Auth { status, detail, .. } => {
            assert_eq!(status, 401);
            assert_eq!(detail, "invalid_api_key");
        }
        other => panic!("expected Auth, got {other}"),
    }
}

#[test]
fn an_empty_close_or_tcp_eof_cannot_finish_the_turn() {
    let (result, _) = finish_from_peer(vec![Message::Close(None)], Finalize::STREAM);
    assert!(result.is_err());
    let (result, _) = finish_against(vec![event(committed("item_1"))], Finalize::STREAM, true);
    // The end of the stream, reported as such rather than waited out.
    assert!(
        matches!(result, Err(SttError::Provider { .. })),
        "{result:?}"
    );
}

#[test]
fn a_failed_transcription_for_this_item_is_an_error() {
    let (result, _) = finish_from_peer(
        vec![
            event(committed("item_1")),
            event(json!({
                "type": "conversation.item.input_audio_transcription.failed",
                "item_id": "item_1",
                "error": {"code": "audio_unintelligible", "message": PRIVATE}
            })),
        ],
        Finalize::STREAM,
    );
    let error = result.unwrap_err();
    assert!(
        error.to_string().contains("audio_unintelligible"),
        "{error}"
    );
    assert_private(&error);
}

#[test]
fn a_silent_server_is_a_stall_not_a_hang() {
    let budget = Finalize {
        per_frame_ms: 30,
        total_ms: 200,
    };
    let (result, _) = finish_from_peer(vec![event(committed("item_1"))], budget);
    assert!(matches!(
        result,
        Err(SttError::Stalled { waited_ms: 30, .. })
    ));

    // The ceiling holds even where the per-frame wait would allow more.
    let deltas = (0..40)
        .map(|_| event(json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": "item_1", "delta": "x"})))
        .collect::<Vec<_>>();
    let budget = Finalize {
        per_frame_ms: 1_000,
        total_ms: 0,
    };
    let (result, _) = finish_from_peer(deltas, budget);
    assert!(matches!(
        result,
        Err(SttError::Stalled { waited_ms: 0, .. })
    ));
}

#[test]
fn socket_errors_never_expose_remote_content() {
    let errors = [
        WsError::Io(std::io::Error::other(PRIVATE)),
        WsError::Utf8(PRIVATE.to_string()),
        WsError::WriteBufferFull(Box::new(Message::Text(PRIVATE.into()))),
    ];
    for error in errors {
        assert_private(&socket_error("socket read failed", &error));
    }
}

#[test]
fn a_refused_upgrade_maps_by_status_without_echoing_the_body() {
    let body = format!(
        r#"{{"error":{{"message":"Incorrect API key provided: sk-proj-****CRET. {PRIVATE}","code":"invalid_api_key","type":"invalid_request_error"}}}}"#
    );
    let refused = |status: u16| {
        WsError::Http(Box::new(
            tokio_tungstenite::tungstenite::http::Response::builder()
                .status(status)
                .header("retry-after", "7")
                .body(Some(body.clone().into_bytes()))
                .unwrap(),
        ))
    };
    match socket_error("connect", &refused(401)) {
        SttError::Auth {
            status, ref detail, ..
        } => {
            assert_eq!(status, 401);
            assert_eq!(detail, "invalid_api_key");
        }
        other => panic!("expected Auth, got {other}"),
    }
    assert!(matches!(
        socket_error("connect", &refused(429)),
        SttError::RateLimited {
            retry_after_s: Some(7),
            ..
        }
    ));
    for status in [401, 403, 429, 500] {
        assert_private(&socket_error("connect", &refused(status)));
    }
}

#[test]
fn the_request_carries_a_sensitive_bearer_header_and_no_beta_header() {
    let request =
        authorized_request("wss://api.openai.com/v1/realtime?intent=transcription", KEY).unwrap();
    let auth = request.headers().get(header::AUTHORIZATION).unwrap();
    assert_eq!(auth.to_str().unwrap(), format!("Bearer {KEY}"));
    assert!(auth.is_sensitive());
    assert!(!format!("{:?}", request.headers()).contains(KEY));
    assert!(request.headers().get("openai-beta").is_none());
    assert_eq!(request.uri().query(), Some("intent=transcription"));
}

#[test]
fn a_key_that_cannot_be_a_header_fails_without_echoing_it() {
    let error =
        authorized_request("wss://api.openai.com/v1/realtime", "sk-bad\nSECRET").unwrap_err();
    assert!(!rendered(&error).contains("SECRET"));
}

/// What the local peer saw of one replayed session.
#[derive(Default)]
struct Seen {
    authorization: Option<String>,
    beta_header: bool,
    path_and_query: String,
    first: Option<Value>,
    appended_samples: usize,
    appends_before_ack: usize,
}

/// The whole replay path — connect, update, append, commit, finish —
/// against a local peer that refuses to acknowledge the update until
/// audio has arrived. A client that waited for `session.updated`
/// before appending would hang here (D10).
// The handshake callback's `Result<Response, ErrorResponse>` is
// tungstenite's signature, not ours to shrink.
#[allow(clippy::result_large_err)]
fn replay_against_peer(
    samples: Vec<f32>,
    on_update: Vec<Message>,
    on_commit: Vec<Message>,
) -> (Result<String, SttError>, Seen) {
    runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let seen = Arc::new(Mutex::new(Seen::default()));
            let peer_seen = seen.clone();
            let peer = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let headers_seen = peer_seen.clone();
                let mut socket = tokio_tungstenite::accept_hdr_async(
                    stream,
                    move |request: &ServerRequest, response: Response| {
                        let mut seen = headers_seen.lock().unwrap();
                        seen.authorization = request
                            .headers()
                            .get(header::AUTHORIZATION)
                            .map(|v| v.to_str().unwrap().to_string());
                        seen.beta_header = request.headers().contains_key("openai-beta");
                        seen.path_and_query = request.uri().to_string();
                        Ok(response)
                    },
                )
                .await
                .unwrap();
                socket
                    .send(event(json!({"type": "session.created", "session": {}})))
                    .await
                    .unwrap();
                let mut acked = false;
                let mut on_update = Some(on_update);
                let mut on_commit = Some(on_commit);
                while let Some(Ok(frame)) = socket.next().await {
                    let Some(body) = frame_json(&frame) else {
                        continue;
                    };
                    let value: Value = serde_json::from_str(&body).unwrap();
                    // Decide under the lock, send after it: a guard
                    // held across an await makes the task non-Send.
                    let replies = {
                        let mut seen = peer_seen.lock().unwrap();
                        if seen.first.is_none() {
                            seen.first = Some(value);
                            let replies = on_update.take().unwrap_or_default();
                            acked |= !replies.is_empty();
                            replies
                        } else {
                            match value["type"].as_str().unwrap() {
                                "input_audio_buffer.append" => {
                                    let bytes = base64::engine::general_purpose::STANDARD
                                        .decode(value["audio"].as_str().unwrap())
                                        .unwrap();
                                    seen.appended_samples += bytes.len() / 2;
                                    if acked {
                                        vec![]
                                    } else {
                                        seen.appends_before_ack += 1;
                                        acked = true;
                                        vec![event(
                                            json!({"type": "session.updated", "session": {}}),
                                        )]
                                    }
                                }
                                "input_audio_buffer.commit" => on_commit.take().unwrap_or_default(),
                                other => panic!("unexpected client event {other}"),
                            }
                        }
                    };
                    for reply in replies {
                        // A client that has already failed the session
                        // may be gone; its result says so, not this.
                        if socket.send(reply).await.is_err() {
                            return;
                        }
                    }
                }
            });
            let url = realtime_url(&format!("http://{address}/v1/")).unwrap();
            let update = session_update("gpt-live-transcribe", &["Hark".to_string()]);
            let result = run(&url, KEY, update, samples, Finalize::REPLAY).await;
            peer.await.unwrap();
            let seen = std::mem::take(&mut *seen.lock().unwrap());
            (result, seen)
        })
        .await
        .expect("local WebSocket replay must terminate")
    })
}

#[test]
fn a_replay_speaks_the_whole_protocol_without_waiting_for_the_ack() {
    let samples = vec![0.25f32; CHUNK_SAMPLES * 2 + 100];
    let (result, seen) = replay_against_peer(
        samples,
        vec![],
        vec![
            event(committed("item_1")),
            event(completed("item_1", "Replayed.")),
        ],
    );
    assert_eq!(result.unwrap(), "Replayed.");
    assert_eq!(
        seen.authorization.as_deref(),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(!seen.beta_header);
    assert_eq!(seen.path_and_query, "/v1/realtime?intent=transcription");
    assert_eq!(
        seen.first,
        Some(session_update("gpt-live-transcribe", &["Hark".to_string()]))
    );
    assert_eq!(seen.appended_samples, CHUNK_SAMPLES * 2 + 100);
    assert!(
        seen.appends_before_ack >= 1,
        "audio must not wait for session.updated"
    );
}

#[test]
fn a_short_replay_is_padded_to_the_minimum_commit() {
    let (result, seen) = replay_against_peer(
        vec![0.25f32; 10],
        vec![],
        vec![event(committed("item_1")), event(completed("item_1", ""))],
    );
    assert_eq!(result.unwrap(), "");
    assert_eq!(seen.appended_samples, MIN_COMMIT_SAMPLES);
}

#[test]
fn a_refused_key_is_an_auth_error_that_never_quotes_it() {
    // CP0's shape: the handshake succeeds, the update is answered by
    // an error event quoting a masked key, then close code 3000.
    let (result, _) = replay_against_peer(
        vec![0.25f32; CHUNK_SAMPLES],
        vec![event(json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "code": "invalid_api_key",
                "message": format!("Incorrect API key provided: sk-proj-****CRET. {PRIVATE}"),
                "param": null,
            }
        }))],
        // The close is held back to the commit, the client's last
        // write: a peer that has sent Close rejects any data after it.
        vec![Message::Close(Some(CloseFrame {
            code: CloseCode::from(3000),
            reason: "invalid_request_error.invalid_api_key".into(),
        }))],
    );
    let error = result.unwrap_err();
    match &error {
        SttError::Auth { status, detail, .. } => {
            assert_eq!(*status, 401);
            assert_eq!(detail, "invalid_api_key");
        }
        other => panic!("expected Auth, got {other}"),
    }
    assert_private(&error);
}

#[test]
fn an_error_in_answer_to_the_update_fails_the_session() {
    // D10: audio is appended before the ack, so a rejected update
    // surfaces from a later read rather than from connect.
    let (result, _) = replay_against_peer(
        vec![0.25f32; CHUNK_SAMPLES * 3],
        vec![event(json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "code": "integer_below_min_value",
                "param": "session.audio.input.format.rate",
                "message": PRIVATE,
            }
        }))],
        vec![
            event(committed("item_1")),
            event(completed("item_1", "too late")),
        ],
    );
    let error = result.unwrap_err();
    let text = error.to_string();
    assert!(text.contains("before the session was configured"), "{text}");
    assert!(text.contains("integer_below_min_value"), "{text}");
    assert_private(&error);
}

/// What a local peer on its own thread does with one streaming session.
#[derive(Default)]
struct Script {
    /// Sent as soon as the socket is up, before anything is read.
    on_connect: Vec<Message>,
    /// Sent in answer to the client's commit.
    on_commit: Vec<Message>,
}

/// A streaming session against a local peer with its own thread and
/// runtime, so the session's own `block_on` drives the client exactly
/// as the pipeline worker does: in bursts, with the runtime idle in
/// between. Returns once `on_connect` has been sent and has had time
/// to land in the client's receive buffer. Drop the session before
/// joining the peer.
fn streamed_session(script: Script) -> (OpenAiLiveSession, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (opened, sent) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream.set_nonblocking(true).unwrap();
        runtime().block_on(async move {
            let stream = tokio::net::TcpStream::from_std(stream).unwrap();
            let mut socket = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
            for frame in script.on_connect {
                socket.send(frame).await.unwrap();
            }
            opened.send(()).unwrap();
            let mut on_commit = Some(script.on_commit);
            while let Some(Ok(frame)) = socket.next().await {
                let commit = frame_json(&frame)
                    .and_then(|body| serde_json::from_str::<Value>(&body).ok())
                    .is_some_and(|v| v["type"] == "input_audio_buffer.commit");
                if commit {
                    for reply in on_commit.take().unwrap_or_default() {
                        if socket.send(reply).await.is_err() {
                            return;
                        }
                    }
                }
            }
        })
    });
    let runtime = Arc::new(runtime());
    let socket = runtime.block_on(async {
        let stream = tokio::net::TcpStream::connect(address).await.unwrap();
        WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None).await
    });
    sent.recv_timeout(Duration::from_secs(5))
        .expect("the peer sends its opening frames");
    // Sent is not yet received; loopback still takes a moment.
    std::thread::sleep(Duration::from_millis(50));
    let session = OpenAiLiveSession {
        runtime,
        socket: Some(socket),
        api_key: KEY.into(),
        turn: Turn::default(),
        pending: Vec::new(),
        appended: 0,
    };
    (session, peer)
}

fn delta() -> Message {
    event(
        json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": "item_1", "delta": "x"}),
    )
}

#[test]
fn an_empty_streamed_transcript_hands_the_dictation_to_the_fallback() {
    // Streamed audio is not gain-normalized; the fallback's is. On a
    // quiet mic a blank stream reported as success lost the dictation
    // as "empty transcript" with the fallback never asked.
    use crate::LiveSession;
    for blank in ["", "  \n"] {
        let (mut session, peer) = streamed_session(Script {
            on_commit: vec![
                event(committed("item_1")),
                event(completed("item_1", blank)),
            ],
            ..Default::default()
        });
        session.push(&vec![0.01; CHUNK_SAMPLES]).unwrap();
        let error = session
            .finish()
            .err()
            .expect("a blank stream must not count as a transcript");
        assert!(matches!(error, SttError::Provider { .. }), "{error}");
        assert!(error.to_string().contains("empty transcript"), "{error}");
        drop(session);
        peer.join().unwrap();
    }

    let (mut session, peer) = streamed_session(Script {
        on_commit: vec![
            event(committed("item_1")),
            event(completed("item_1", "Said.")),
        ],
        ..Default::default()
    });
    session.push(&vec![0.01; CHUNK_SAMPLES]).unwrap();
    assert_eq!(session.finish().unwrap().text, "Said.");
    drop(session);
    peer.join().unwrap();
}

#[test]
fn request_ms_is_the_wait_after_release_not_the_hold() {
    // Timed from the session opening, a 13.8 s hold logged as
    // "finalise took 13772 ms" on a dictation injected 1.7 s after
    // release, and history showed it as the STT time.
    use crate::LiveSession;
    let (mut session, peer) = streamed_session(Script {
        on_commit: vec![
            event(committed("item_1")),
            event(completed("item_1", "Said.")),
        ],
        ..Default::default()
    });
    session.push(&vec![0.01; CHUNK_SAMPLES]).unwrap();
    let hold = Duration::from_millis(600);
    std::thread::sleep(hold);
    let transcript = session.finish().unwrap();
    assert!(
        transcript.request_ms < hold.as_millis(),
        "request_ms {} includes the hold",
        transcript.request_ms
    );
    drop(session);
    peer.join().unwrap();
}

#[test]
fn a_drain_sees_frames_that_landed_outside_the_runtime() {
    // The worker enters the adapter's runtime only inside push, so
    // frames reach the kernel while nothing polls the driver. Without
    // the driver turn before `now_or_never`, tokio's readiness is
    // still empty and the drain reads nothing.
    use crate::LiveSession;
    let (mut session, peer) = streamed_session(Script {
        on_connect: vec![
            event(json!({"type": "session.updated", "session": {}})),
            delta(),
            delta(),
        ],
        ..Default::default()
    });
    session.push(&[]).unwrap();
    assert_eq!(session.turn.frames(), 3);
    assert_eq!(session.turn.deltas(), 2);
    assert!(session.turn.configured());
    drop(session);
    peer.join().unwrap();
}

#[test]
fn an_empty_drain_never_waits_on_a_timer() {
    // `timeout(Duration::ZERO, ..)` parked until the next 1 ms timer
    // tick on every empty drain, which Windows stretches to ~15.6 ms:
    // these 100 drains took about 1.5 s. Without a timer they take
    // microseconds; the bound is loose so a loaded CI box cannot trip it.
    use crate::LiveSession;
    let (mut session, peer) = streamed_session(Script::default());
    let started = Instant::now();
    for _ in 0..100 {
        session.push(&[]).unwrap();
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(500),
        "100 empty drains took {elapsed:?}"
    );
    drop(session);
    peer.join().unwrap();
}

#[test]
fn a_failed_send_reports_the_servers_refusal_instead() {
    // D10's race, decided for the server: the refusal is already in
    // the receive buffer when a write fails because it hung up.
    let (mut session, peer) = streamed_session(Script {
        on_connect: vec![
            event(json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "code": "invalid_api_key",
                    "message": format!("Incorrect API key provided: sk-proj-****CRET. {PRIVATE}"),
                }
            })),
            Message::Close(Some(CloseFrame {
                code: CloseCode::from(3000),
                reason: "invalid_request_error.invalid_api_key".into(),
            })),
        ],
        ..Default::default()
    });
    let send_error = fail("append send failed: I/O ConnectionReset");
    let socket = session.socket.as_mut().unwrap();
    let reported =
        session
            .runtime
            .block_on(explain_send_failure(socket, &mut session.turn, send_error));
    match &reported {
        SttError::Auth { status, detail, .. } => {
            assert_eq!(*status, 401);
            assert_eq!(detail, "invalid_api_key");
        }
        other => panic!("expected Auth, got {other}"),
    }
    assert_private(&reported);
    drop(session);
    peer.join().unwrap();
}

#[test]
fn a_failed_send_with_nothing_from_the_server_keeps_its_own_error() {
    let (mut session, peer) = streamed_session(Script {
        on_connect: vec![delta()],
        ..Default::default()
    });
    let socket = session.socket.as_mut().unwrap();
    let reported = session.runtime.block_on(explain_send_failure(
        socket,
        &mut session.turn,
        fail("append send failed: connection closed"),
    ));
    assert!(
        reported.to_string().contains("append send failed"),
        "{reported}"
    );
    // What it did read on the way was still recorded.
    assert_eq!(session.turn.deltas(), 1);
    drop(session);
    peer.join().unwrap();
}
