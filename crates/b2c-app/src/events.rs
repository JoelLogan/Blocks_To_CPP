//! App events: notifications that belong to no command
//! (`docs/spec/02-architecture.md` §2.5.3), pushed on the channel the frontend
//! gives to `app_subscribe`.
//!
//! There is one subscription at a time: a later `app_subscribe` replaces the
//! channel. Events sent while nobody is subscribed are dropped, which is
//! harmless: `toolchain_list` reports `discovering`, `settings_get` returns
//! the notices, and the settings notices from startup are also sent to the
//! first subscriber.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use b2c_ipc::EventSink;
use b2c_ipc::dto::{AppEvent, SettingsNotice};

/// The app-event channel (shared with the file watcher and the background
/// tasks).
#[derive(Default)]
pub(crate) struct AppEvents {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    sink: Option<Arc<dyn EventSink<AppEvent>>>,
    /// The notices from loading the settings, until the first subscriber
    /// gets them.
    startup_notices: Option<Vec<SettingsNotice>>,
}

impl std::fmt::Debug for AppEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppEvents")
            .field("subscribed", &self.lock().sink.is_some())
            .finish()
    }
}

impl AppEvents {
    /// The channel, with the settings notices from startup to deliver to the
    /// first subscriber (none when empty).
    pub(crate) fn new(startup_notices: Vec<SettingsNotice>) -> Self {
        Self {
            state: Mutex::new(State {
                sink: None,
                startup_notices: (!startup_notices.is_empty()).then_some(startup_notices),
            }),
        }
    }

    /// Makes `sink` the channel, replacing an earlier one, and sends it the
    /// startup notices if no subscriber has had them yet.
    pub(crate) fn subscribe(&self, sink: Arc<dyn EventSink<AppEvent>>) {
        let notices = {
            let mut state = self.lock();
            state.sink = Some(sink);
            state.startup_notices.take()
        };
        if let Some(notices) = notices {
            self.send(AppEvent::SettingsNotice { notices });
        }
    }

    /// Sends `event` to the subscriber. Returns whether it was delivered: no
    /// subscriber, or one that has gone away (which is then forgotten), gives
    /// `false`. The sink is called without holding the lock.
    pub(crate) fn send(&self, event: AppEvent) -> bool {
        let Some(sink) = self.lock().sink.clone() else {
            return false;
        };
        let delivered = sink.send(event);
        if !delivered {
            self.forget(&sink);
        }
        delivered
    }

    /// Forgets `sink` if it is still the subscriber.
    fn forget(&self, sink: &Arc<dyn EventSink<AppEvent>>) {
        let mut state = self.lock();
        if state
            .sink
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, sink))
        {
            state.sink = None;
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use b2c_ipc::dto::NoticeReason;
    use b2c_ipc::sink::testing::RecordingSink;

    use super::*;

    fn notice() -> SettingsNotice {
        SettingsNotice {
            key: String::from("console.scrollbackLines"),
            reason: NoticeReason::InvalidValue,
        }
    }

    #[test]
    fn the_first_subscriber_gets_the_startup_notices() {
        let events = AppEvents::new(vec![notice()]);
        assert!(!events.send(AppEvent::CloseRequested));
        let first = Arc::new(RecordingSink::new());
        events.subscribe(first.clone());
        assert_eq!(
            first.events(),
            [AppEvent::SettingsNotice {
                notices: vec![notice()]
            }]
        );
        let second = Arc::new(RecordingSink::new());
        events.subscribe(second.clone());
        assert!(second.is_empty());
        assert!(events.send(AppEvent::CloseRequested));
        assert_eq!(second.events(), [AppEvent::CloseRequested]);
        assert_eq!(first.len(), 1);
    }

    #[test]
    fn a_closed_channel_is_forgotten() {
        let events = AppEvents::new(Vec::new());
        let sink = Arc::new(RecordingSink::new());
        events.subscribe(sink.clone());
        assert!(sink.is_empty());
        assert!(events.lock().sink.is_some());
        sink.close();
        assert!(!events.send(AppEvent::CloseRequested));
        assert!(events.lock().sink.is_none());
    }
}
