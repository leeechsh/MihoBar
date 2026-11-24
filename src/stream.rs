//! The API's streams (`/traffic`, `/memory`, `/logs`, `/connections`) over
//! WebSocket, reconnecting with a backoff of 1 to 8 seconds (ClashBar's
//! `StreamCoordinator`). The socket runs on the runtime's threads; its
//! messages come to the engine thread through a channel.

use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_tungstenite::tungstenite::Message;

/// What a stream reports.
#[derive(Debug)]
pub enum StreamEvent {
    Payload(String),
    /// The socket closed or failed; it reconnects by itself.
    Disconnected(String),
}

/// A running stream; dropping it stops it.
pub struct Stream {
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

const BASE_DELAY: Duration = Duration::from_secs(1);
const MAX_DELAY: Duration = Duration::from_secs(8);

/// The engine's runtime, kept from a task: a stream can open from an
/// effect a press ran, outside the runtime's context.
static RUNTIME: std::sync::OnceLock<tokio::runtime::Handle> = std::sync::OnceLock::new();

/// Keeps the runtime of the calling task (call from a task).
pub fn remember_runtime() {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let _ = RUNTIME.set(handle);
    }
}

/// Opens `url` and keeps it open; its messages come to the receiver.
pub fn open(url: String) -> (Stream, UnboundedReceiver<StreamEvent>) {
    let (tx, rx) = unbounded_channel();
    let task = match tokio::runtime::Handle::try_current().ok().or_else(|| RUNTIME.get().cloned()) {
        Some(handle) => handle.spawn(run(url, tx)),
        None => tokio::spawn(run(url, tx)),
    };
    (Stream { task }, rx)
}

async fn run(url: String, tx: UnboundedSender<StreamEvent>) {
    let mut attempts: u32 = 0;
    loop {
        match tokio_tungstenite::connect_async(url.as_str()).await {
            Ok((mut socket, _)) => {
                attempts = 0;
                loop {
                    match socket.next().await {
                        Some(Ok(Message::Text(text))) => {
                            if tx.send(StreamEvent::Payload(text.to_string())).is_err() {
                                return;
                            }
                        }
                        Some(Ok(Message::Binary(bytes))) => {
                            let text = String::from_utf8_lossy(&bytes).into_owned();
                            if tx.send(StreamEvent::Payload(text)).is_err() {
                                return;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            if tx.send(StreamEvent::Disconnected("closed".into())).is_err() {
                                return;
                            }
                            break;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            if tx.send(StreamEvent::Disconnected(e.to_string())).is_err() {
                                return;
                            }
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                if tx.send(StreamEvent::Disconnected(e.to_string())).is_err() {
                    return;
                }
            }
        }
        if tx.is_closed() {
            return;
        }
        let delay = BASE_DELAY.saturating_mul(1 << attempts.min(3)).min(MAX_DELAY);
        attempts += 1;
        tokio::time::sleep(delay).await;
    }
}
