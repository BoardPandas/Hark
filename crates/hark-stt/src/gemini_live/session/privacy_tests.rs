//! The session against a local WebSocket peer: errors never carry what the
//! server sent, and only an explicit completion finishes a turn.

use super::*;
use serde_json::json;
use tokio_tungstenite::tungstenite::{
    protocol::{frame::coding::CloseCode, CloseFrame},
    Error,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

#[test]
fn socket_errors_and_close_reasons_never_expose_remote_content() {
    const PRIVATE: &str = "private_transcript_and_key";
    let errors = [
        Error::Io(std::io::Error::other(PRIVATE)),
        Error::Utf8(PRIVATE.to_string()),
        Error::WriteBufferFull(Box::new(Message::Text(PRIVATE.into()))),
    ];
    for error in errors {
        let safe = socket_error("socket read failed", &error);
        assert!(!format!("{safe} {safe:?}").contains(PRIVATE));
    }
    let safe = setup_closed(Some(CloseFrame {
        code: CloseCode::Policy,
        reason: PRIVATE.into(),
    }));
    assert!(safe.to_string().contains("1008"));
    assert!(!format!("{safe} {safe:?}").contains(PRIVATE));
}

/// Exercise the real finalizer with a local WebSocket peer. No TLS,
/// provider key, or external service is involved.
fn finish_from_peer(
    frames: Vec<Message>,
    wait_for_close: bool,
    budget: Finalize,
    segments: Vec<String>,
) -> Result<String, SttError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                use tokio_tungstenite::tungstenite::protocol::Role;

                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let peer = tokio::spawn(async move {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut socket =
                        WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
                    // The real finalizer must signal both end markers.
                    for expected in [activity_end_message(), audio_stream_end_message()] {
                        let frame = socket.next().await.unwrap().unwrap();
                        let actual: Value =
                            serde_json::from_str(&frame_json(&frame).unwrap()).unwrap();
                        assert_eq!(actual, expected);
                    }
                    for frame in frames {
                        socket.send(frame).await.unwrap();
                    }
                    if wait_for_close {
                        assert!(matches!(socket.next().await, Some(Ok(Message::Close(_)))));
                    }
                });
                let stream = tokio::net::TcpStream::connect(address).await.unwrap();
                let mut socket = WebSocketStream::from_raw_socket(
                    MaybeTlsStream::Plain(stream),
                    Role::Client,
                    None,
                )
                .await;
                let result = finish(&mut socket, TranscribeMode::Verbatim, segments, budget).await;
                drop(socket);
                peer.await.unwrap();
                result
            })
            .await
            .expect("local WebSocket finalization must terminate")
        })
}

fn text_frame(content: Value) -> Message {
    Message::Text(json!({"serverContent": content}).to_string().into())
}

#[test]
fn a_close_before_turn_completion_rejects_partial_transcripts() {
    for code in [CloseCode::Normal, CloseCode::Error, CloseCode::Policy] {
        let error = finish_from_peer(
            vec![Message::Close(Some(CloseFrame {
                code,
                reason: "private_provider_content".into(),
            }))],
            false,
            Finalize::STREAM,
            vec!["Only the first sentence.".into()],
        )
        .unwrap_err();
        assert!(error.to_string().contains(&u16::from(code).to_string()));
        assert!(!format!("{error} {error:?}").contains("private_provider_content"));
    }
}

#[test]
fn an_empty_close_or_tcp_eof_cannot_complete_a_partial_transcript() {
    for frames in [vec![Message::Close(None)], vec![]] {
        assert!(finish_from_peer(
            frames,
            false,
            Finalize::STREAM,
            vec!["Only the first sentence.".into()],
        )
        .is_err());
    }
}

#[test]
fn explicit_completion_keeps_all_finalized_segments() {
    for marker in ["generationComplete", "turnComplete"] {
        let text = finish_from_peer(
            vec![
                text_frame(json!({"inputTranscription": {"text": "The second sentence."}})),
                text_frame(json!({marker: true})),
            ],
            true,
            Finalize::STREAM,
            vec!["The first sentence.".into()],
        )
        .unwrap();
        assert_eq!(text, "The first sentence. The second sentence.");
    }
}

#[test]
fn completion_and_final_text_in_one_frame_keep_both_facts() {
    let text = finish_from_peer(
        vec![text_frame(json!({
            "inputTranscription": {"text": "The last sentence."},
            "turnComplete": true
        }))],
        true,
        Finalize::STREAM,
        vec!["The first sentence.".into()],
    )
    .unwrap();
    assert_eq!(text, "The first sentence. The last sentence.");
}

#[test]
fn meeting_silence_stays_empty_but_disconnect_is_an_error() {
    let budget = Finalize {
        per_frame_ms: 20,
        total_ms: 100,
        quiet_is_empty: true,
    };
    assert_eq!(finish_from_peer(vec![], true, budget, vec![]).unwrap(), "");
    assert!(finish_from_peer(vec![Message::Close(None)], false, budget, vec![],).is_err());
}
