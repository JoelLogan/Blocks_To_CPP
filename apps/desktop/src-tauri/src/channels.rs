//! Tauri channels as the backend's sinks (`docs/spec/02-architecture.md`
//! §2.5.3: "Streaming uses Tauri `Channel`s").
//!
//! Every push from the backend to the webview goes through a channel that the
//! frontend passed as a command argument (`onEvent`, `onOutput`), never through
//! Tauri's global events: the window has no `core:event` permission.
//!
//! * [`EventChannel`] sends JSON messages (app, build and run events).
//! * [`ByteChannel`] sends program output as raw bytes
//!   (`InvokeResponseBody::Raw`, an `ArrayBuffer` in the webview). Tauri
//!   delivers a large message through its `plugin:__TAURI_CHANNEL__|fetch`
//!   command, which the isolation hook allows.
//!
//! A send that fails (the webview is gone) returns `false`; the sessions then
//! stop sending to that channel.

use b2c_ipc::{ByteSink, EventSink};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};

/// A JSON channel to the webview, as an [`EventSink`].
pub(crate) struct EventChannel<T> {
    channel: Channel<T>,
}

impl<T> EventChannel<T> {
    /// Wraps a channel received as a command argument.
    pub(crate) fn new(channel: Channel<T>) -> Self {
        Self { channel }
    }
}

impl<T: Serialize + Send + Sync> EventSink<T> for EventChannel<T> {
    fn send(&self, event: T) -> bool {
        self.channel.send(event).is_ok()
    }
}

/// A raw byte channel to the webview, as a [`ByteSink`].
pub(crate) struct ByteChannel {
    channel: Channel<InvokeResponseBody>,
}

impl ByteChannel {
    /// Wraps a channel received as a command argument.
    pub(crate) fn new(channel: Channel<InvokeResponseBody>) -> Self {
        Self { channel }
    }
}

impl ByteSink for ByteChannel {
    fn send(&self, bytes: Vec<u8>) -> bool {
        self.channel.send(InvokeResponseBody::Raw(bytes)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use b2c_ipc::dto::{BuildEvent, BuildOutcome};

    use super::*;

    /// A channel whose messages land in a list, and one that always fails.
    fn recording<T>() -> (Channel<T>, Arc<Mutex<Vec<InvokeResponseBody>>>) {
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            sink.lock().unwrap().push(body);
            Ok(())
        });
        (channel, received)
    }

    #[test]
    fn json_events_are_sent_as_their_ipc_json() {
        let (channel, received) = recording::<BuildEvent>();
        let sink: Arc<dyn EventSink<BuildEvent>> = Arc::new(EventChannel::new(channel));
        assert!(sink.send(BuildEvent::finished(BuildOutcome::Built, None, 12)));
        let bodies = received.lock().unwrap();
        let [InvokeResponseBody::Json(json)] = bodies.as_slice() else {
            panic!("expected one JSON message, got {bodies:?}");
        };
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(value["kind"], "finished");
        assert_eq!(value["outcome"], "built");
        assert_eq!(value["elapsedMs"], 12);
    }

    #[test]
    fn output_is_sent_as_raw_bytes_of_any_size() {
        let (channel, received) = recording::<InvokeResponseBody>();
        let sink: Arc<dyn ByteSink> = Arc::new(ByteChannel::new(channel));
        let big = vec![b'x'; 64 * 1024];
        assert!(sink.send(b"hello".to_vec()));
        assert!(sink.send(big.clone()));
        let bodies = received.lock().unwrap();
        assert_eq!(bodies.len(), 2);
        assert!(matches!(&bodies[0], InvokeResponseBody::Raw(bytes) if bytes == b"hello"));
        assert!(matches!(&bodies[1], InvokeResponseBody::Raw(bytes) if *bytes == big));
    }

    /// Fetches a queued channel message the way the webview does
    /// (`plugin:__TAURI_CHANNEL__|fetch` with the message's ID in a header).
    fn fetch(webview: &tauri::WebviewWindow<tauri::test::MockRuntime>, id: u32) -> InvokeResponseBody {
        use tauri::ipc::{CallbackFn, InvokeBody};
        use tauri::webview::InvokeRequest;

        let mut headers = tauri::http::HeaderMap::new();
        headers.insert("Tauri-Channel-Id", id.to_string().parse().unwrap());
        let url = if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        };
        tauri::test::get_ipc_response(
            webview,
            InvokeRequest {
                cmd: String::from("plugin:__TAURI_CHANNEL__|fetch"),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: url.parse().unwrap(),
                body: InvokeBody::Json(serde_json::Value::Null),
                headers,
                invoke_key: String::from(tauri::test::INVOKE_KEY),
            },
        )
        .unwrap()
    }

    #[test]
    fn large_messages_reach_the_webview_through_the_channel_fetch_command() {
        use std::str::FromStr as _;

        use tauri::ipc::JavaScriptChannelId;

        let app = tauri::test::mock_app();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        let channel = |id: &str| JavaScriptChannelId::from_str(id).unwrap();

        // Raw output of 1 KiB or more, and JSON of 8 KiB or more, is queued
        // and fetched by the webview instead of being evaluated inline.
        let bytes = ByteChannel::new(channel("__CHANNEL__:5").channel_on(webview.as_ref().clone()));
        let output: Vec<u8> = (0..20_000_u32).map(|i| u8::try_from(i % 251).unwrap()).collect();
        assert!(bytes.send(output.clone()));
        let events = EventChannel::<serde_json::Value>::new(
            channel("__CHANNEL__:6").channel_on(webview.as_ref().clone()),
        );
        let text = "e".repeat(10_000);
        assert!(events.send(serde_json::json!({ "kind": "diagnostics", "text": text })));

        let InvokeResponseBody::Raw(fetched) = fetch(&webview, 0) else {
            panic!("raw output must stay raw");
        };
        assert_eq!(fetched, output);
        let InvokeResponseBody::Json(json) = fetch(&webview, 1) else {
            panic!("events are JSON");
        };
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["text"].as_str().map(str::len), Some(10_000));
    }

    #[test]
    fn a_failing_channel_reports_false() {
        let channel: Channel<InvokeResponseBody> = Channel::new(|_| Err(tauri::Error::WebviewNotFound));
        assert!(!ByteChannel::new(channel).send(b"lost".to_vec()));
        let channel: Channel<BuildEvent> = Channel::new(|_| Err(tauri::Error::WebviewNotFound));
        assert!(!EventChannel::new(channel).send(BuildEvent::finished(BuildOutcome::Cancelled, None, 0)));
    }
}
