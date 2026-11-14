//! A streamed session driven the way the pipeline worker drives it: in
//! bursts of `block_on`, with the runtime idle in between.

use super::*;
use crate::LiveSession;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn transcription(text: &str) -> Message {
    Message::Text(
        json!({"serverContent": {"inputTranscription": {"text": text}}})
            .to_string()
            .into(),
    )
}

/// A streaming session against a local peer with its own thread and
/// runtime, so the session's own `block_on` drives the client exactly as
/// the pipeline worker does: in bursts, with the runtime idle in between.
/// The peer sends `on_connect`, then reads and discards whatever the
/// client pushes. Returns once `on_connect` has had time to land in the
/// client's receive buffer. Drop the session before joining the peer.
fn streamed_session(on_connect: Vec<Message>) -> (GeminiLiveSession, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (opened, sent) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream.set_nonblocking(true).unwrap();
        runtime().block_on(async move {
            let stream = tokio::net::TcpStream::from_std(stream).unwrap();
            let mut socket = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
            for frame in on_connect {
                socket.send(frame).await.unwrap();
            }
            opened.send(()).unwrap();
            while let Some(Ok(_)) = socket.next().await {}
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
    let session = GeminiLiveSession {
        runtime,
        socket: Some(socket),
        mode: TranscribeMode::Verbatim,
        pushed: 0,
        segments: Vec::new(),
        turn_complete: false,
    };
    (session, peer)
}

#[test]
fn a_drain_sees_frames_that_landed_outside_the_runtime() {
    // The worker enters the adapter's runtime only inside push, so
    // frames reach the kernel while nothing polls the driver. Without the
    // driver turn before `now_or_never`, tokio's readiness is still empty
    // and the drain reads nothing.
    let (mut session, peer) =
        streamed_session(vec![transcription("First."), transcription("Second.")]);
    session.push(&vec![0.01; CHUNK_SAMPLES]).unwrap();
    assert_eq!(session.segments, ["First.", "Second."]);
    drop(session);
    peer.join().unwrap();
}

#[test]
fn request_ms_is_the_wait_after_release_not_the_hold() {
    // Timed from the session opening, every live dictation reported its
    // whole hold as the finalise wait and as the STT time in history.
    let (mut session, peer) = streamed_session(vec![
        transcription("Said."),
        Message::Text(r#"{"serverContent":{"turnComplete":true}}"#.into()),
    ]);
    session.push(&vec![0.01; CHUNK_SAMPLES]).unwrap();
    let hold = Duration::from_millis(600);
    std::thread::sleep(hold);
    let transcript = session.finish().unwrap();
    assert_eq!(transcript.text, "Said.");
    assert!(
        transcript.request_ms < hold.as_millis(),
        "request_ms {} includes the hold",
        transcript.request_ms
    );
    drop(session);
    peer.join().unwrap();
}

#[test]
fn an_empty_drain_never_waits_on_a_timer() {
    // `timeout(Duration::ZERO, ..)` parked until the next 1 ms timer tick
    // on every empty drain, which Windows stretches to ~15.6 ms: these 100
    // pushes drain twice each and took about 3 s. Without a timer they
    // take milliseconds; the bound is loose so a loaded CI box cannot
    // trip it.
    let (mut session, peer) = streamed_session(Vec::new());
    let chunk = vec![0.01; CHUNK_SAMPLES];
    let started = Instant::now();
    for _ in 0..100 {
        session.push(&chunk).unwrap();
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(500),
        "100 pushes took {elapsed:?}"
    );
    drop(session);
    peer.join().unwrap();
}
