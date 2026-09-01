use std::collections::HashMap;
use std::sync::Mutex;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use thelemail_api::Net;
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StreamFrame {
    pub stream: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenStreamArgs {
    pub url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseStreamArgs {
    pub stream_id: String,
}

#[derive(Default)]
pub struct Streams {
    open: Mutex<HashMap<String, oneshot::Sender<()>>>,
}

#[tauri::command]
pub async fn realtime_open(
    app: AppHandle,
    net: State<'_, Net>,
    streams: State<'_, Streams>,
    args: OpenStreamArgs,
) -> Result<String, String> {
    let stream_id = Uuid::new_v4().to_string();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    streams
        .open
        .lock()
        .expect("streams")
        .insert(stream_id.clone(), cancel_tx);

    let response = net
        .stream_events(&args.url)
        .await
        .map_err(|e| e.to_string())?;

    let handle = app.clone();
    let id = stream_id.clone();
    tauri::async_runtime::spawn(async move {
        let _ = handle.emit(
            "realtime",
            StreamFrame {
                stream: id.clone(),
                kind: "open",
                data: None,
                id: None,
            },
        );

        let mut body = response.bytes_stream();
        let mut buffer = String::new();
        let mut cancel = cancel_rx;

        loop {
            tokio::select! {
                _ = &mut cancel => break,
                chunk = body.next() => {
                    let Some(chunk) = chunk else { break };
                    let Ok(bytes) = chunk else { break };
                    buffer.push_str(&String::from_utf8_lossy(&bytes));
                    while let Some(split) = find_frame_end(&buffer) {
                        let raw = buffer[..split].to_owned();
                        buffer.drain(..split + frame_sep_len(&buffer, split));
                        if let Some(frame) = parse_frame(&raw, &id) {
                            let _ = handle.emit("realtime", frame);
                        }
                    }
                }
            }
        }

        let _ = handle.emit(
            "realtime",
            StreamFrame {
                stream: id,
                kind: "error",
                data: None,
                id: None,
            },
        );
    });

    Ok(stream_id)
}

#[tauri::command]
pub fn realtime_close(streams: State<'_, Streams>, args: CloseStreamArgs) {
    if let Some(tx) = streams
        .open
        .lock()
        .expect("streams")
        .remove(&args.stream_id)
    {
        let _ = tx.send(());
    }
}

fn find_frame_end(buffer: &str) -> Option<usize> {
    let lf = buffer.find("\n\n");
    let crlf = buffer.find("\r\n\r\n");
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn frame_sep_len(buffer: &str, at: usize) -> usize {
    if buffer[at..].starts_with("\r\n\r\n") {
        4
    } else {
        2
    }
}

fn parse_frame(raw: &str, stream: &str) -> Option<StreamFrame> {
    let mut data = String::new();
    let mut id = None;

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim_start());
        } else if let Some(rest) = line.strip_prefix("id:") {
            id = Some(rest.trim().to_owned());
        }
    }

    if data.is_empty() {
        return None;
    }
    Some(StreamFrame {
        stream: stream.to_owned(),
        kind: "message",
        data: Some(data),
        id,
    })
}
