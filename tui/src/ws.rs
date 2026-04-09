use crate::models::{SseDelta, WsEvent};
use std::{
    io::{BufRead, BufReader, Read},
    sync::mpsc::Sender,
    thread,
};

/// Spawn a thread that reads an SSE response body and sends WsEvent messages
/// through `tx`. This replaces the old Centrifugo WebSocket client.
///
/// The SSE format emitted by the MVS sera binary:
///
/// ```text
/// event: message
/// data: {"delta":"token","session_id":"...","message_id":"..."}
///
/// event: done
/// data: {"status":"complete","usage":{...}}
/// ```
pub fn spawn_sse_thread(
    reader: Box<dyn Read + Send>,
    tx: Sender<WsEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        if let Err(e) = read_sse(reader, &tx) {
            let _ = tx.send(WsEvent::Error(e.to_string()));
        }
    })
}

fn read_sse(
    reader: Box<dyn Read + Send>,
    tx: &Sender<WsEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let buf = BufReader::new(reader);
    let mut current_event: Option<String> = None;

    for line in buf.lines() {
        let line = line?;

        if line.starts_with("event:") {
            let event_name = line["event:".len()..].trim().to_owned();
            current_event = Some(event_name);
        } else if line.starts_with("data:") {
            let data = line["data:".len()..].trim();

            match current_event.as_deref() {
                Some("message") => {
                    if let Ok(delta) = serde_json::from_str::<SseDelta>(data) {
                        if !delta.delta.is_empty() {
                            if tx.send(WsEvent::Token(delta.delta)).is_err() {
                                return Ok(());
                            }
                        }
                    }
                }
                Some("done") => {
                    let _ = tx.send(WsEvent::Done);
                    return Ok(());
                }
                _ => {
                    // Unknown or missing event type — try to detect done by content.
                    if data.contains("\"status\"") {
                        let _ = tx.send(WsEvent::Done);
                        return Ok(());
                    }
                }
            }
        } else if line.is_empty() {
            // Blank line = end of SSE event block; reset event type.
            current_event = None;
        }
    }

    // EOF with no explicit done event — treat as done.
    let _ = tx.send(WsEvent::Done);
    Ok(())
}
