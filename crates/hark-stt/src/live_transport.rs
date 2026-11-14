//! What the two live adapters share: the socket type, how a server frame's
//! JSON is read, the timer-free driver turn, and how each builds its private
//! runtime.
//!
//! Only what [`crate::gemini_live`] and [`crate::openai_live`] do identically
//! lives here. Protocol logic, budgets and error mapping stay per adapter, and
//! so does the runtime itself: [`current_thread_runtime`] builds a new one for
//! each caller, so the two adapters never share an executor.

use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// The WebSocket a live session runs over.
pub type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// The JSON body of a server frame, whichever frame type carried it.
///
/// The Live API sends its JSON in **binary** frames, not text ones, so
/// matching only `Message::Text` silently ignores the entire conversation
/// — including `setupComplete` and the transcript itself. Text is accepted
/// too because nothing in the protocol promises it will stay binary.
pub fn frame_json(frame: &Message) -> Option<String> {
    match frame {
        Message::Text(body) => Some(body.to_string()),
        Message::Binary(bytes) => String::from_utf8(bytes.to_vec()).ok(),
        _ => None,
    }
}

/// Let the runtime's driver look at the socket once, without a timer.
///
/// `yield_now` hands the waker to the current_thread scheduler's defer
/// list, so `block_on` takes its `park_yield` path: one I/O driver turn
/// with a zero timeout, then the deferred waker fires and this future is
/// polled again (tokio 1.53, `runtime/scheduler/current_thread`). That
/// turn is what moves readiness the kernel already has into tokio, so the
/// `now_or_never` reads after it can see frames that landed while the
/// worker was outside the runtime.
pub async fn turn_driver() {
    tokio::task::yield_now().await;
}

/// A new `current_thread` runtime with every driver enabled.
///
/// Each live adapter calls this once and keeps the result to itself, entered
/// only through `block_on` from the pipeline worker thread. It is a
/// constructor, not a shared runtime: nothing here is process-wide.
pub fn current_thread_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}
