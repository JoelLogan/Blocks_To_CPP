//! Where the backend pushes channel messages (`docs/spec/02-architecture.md` §2.5:
//! "Streaming uses Tauri `Channel`s").
//!
//! Build and run sessions and the app-event subscription send through these traits,
//! so they never depend on Tauri: the desktop adapter implements them over
//! `tauri::ipc::Channel`, and tests use the recording sinks in [`testing`].

use std::sync::Arc;

/// A sink for JSON channel messages (build, run and app events).
pub trait EventSink<T>: Send + Sync {
    /// Sends one message. Returns `false` when the receiver is gone (the window
    /// closed or the channel was replaced); the sender should then stop sending.
    fn send(&self, event: T) -> bool;
}

/// A sink for raw byte batches (program output).
pub trait ByteSink: Send + Sync {
    /// Sends one batch. Returns `false` when the receiver is gone.
    fn send(&self, bytes: Vec<u8>) -> bool;
}

impl<T, S: EventSink<T> + ?Sized> EventSink<T> for Arc<S> {
    fn send(&self, event: T) -> bool {
        (**self).send(event)
    }
}

impl<S: ByteSink + ?Sized> ByteSink for Arc<S> {
    fn send(&self, bytes: Vec<u8>) -> bool {
        (**self).send(bytes)
    }
}

/// Sinks that record what they receive, for tests of the sessions that send.
pub mod testing {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use super::{ByteSink, EventSink};

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records every event, until [`close`](Self::close) makes it refuse them.
    #[derive(Debug)]
    pub struct RecordingSink<T> {
        events: Mutex<Vec<T>>,
        open: AtomicBool,
    }

    impl<T> Default for RecordingSink<T> {
        fn default() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
                open: AtomicBool::new(true),
            }
        }
    }

    impl<T> RecordingSink<T> {
        /// An open, empty sink.
        pub fn new() -> Self {
            Self::default()
        }

        /// Simulates a receiver that went away: later sends return `false` and are
        /// not recorded.
        pub fn close(&self) {
            self.open.store(false, Ordering::SeqCst);
        }

        /// Whether the sink still accepts events.
        pub fn is_open(&self) -> bool {
            self.open.load(Ordering::SeqCst)
        }

        /// Removes and returns the events recorded so far.
        pub fn take(&self) -> Vec<T> {
            std::mem::take(&mut *lock(&self.events))
        }

        /// The number of events recorded so far.
        pub fn len(&self) -> usize {
            lock(&self.events).len()
        }

        /// Whether nothing was recorded yet.
        pub fn is_empty(&self) -> bool {
            self.len() == 0
        }
    }

    impl<T: Clone> RecordingSink<T> {
        /// A copy of the events recorded so far.
        pub fn events(&self) -> Vec<T> {
            lock(&self.events).clone()
        }
    }

    impl<T: Send> EventSink<T> for RecordingSink<T> {
        fn send(&self, event: T) -> bool {
            if !self.is_open() {
                return false;
            }
            lock(&self.events).push(event);
            true
        }
    }

    /// Records every byte batch, until [`close`](Self::close) makes it refuse them.
    #[derive(Debug)]
    pub struct RecordingBytes {
        batches: Mutex<Vec<Vec<u8>>>,
        open: AtomicBool,
    }

    impl Default for RecordingBytes {
        fn default() -> Self {
            Self {
                batches: Mutex::new(Vec::new()),
                open: AtomicBool::new(true),
            }
        }
    }

    impl RecordingBytes {
        /// An open, empty sink.
        pub fn new() -> Self {
            Self::default()
        }

        /// Simulates a receiver that went away: later sends return `false` and are
        /// not recorded.
        pub fn close(&self) {
            self.open.store(false, Ordering::SeqCst);
        }

        /// Whether the sink still accepts batches.
        pub fn is_open(&self) -> bool {
            self.open.load(Ordering::SeqCst)
        }

        /// A copy of the batches recorded so far, in order.
        pub fn batches(&self) -> Vec<Vec<u8>> {
            lock(&self.batches).clone()
        }

        /// Every recorded byte, concatenated in order.
        pub fn concat(&self) -> Vec<u8> {
            lock(&self.batches).concat()
        }

        /// Removes and returns the batches recorded so far.
        pub fn take(&self) -> Vec<Vec<u8>> {
            std::mem::take(&mut *lock(&self.batches))
        }
    }

    impl ByteSink for RecordingBytes {
        fn send(&self, bytes: Vec<u8>) -> bool {
            if !self.is_open() {
                return false;
            }
            lock(&self.batches).push(bytes);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::testing::{RecordingBytes, RecordingSink};
    use super::*;

    fn send_all<S: EventSink<u32>>(sink: &S, events: &[u32]) -> Vec<bool> {
        events.iter().map(|&e| sink.send(e)).collect()
    }

    #[test]
    fn recording_sink_keeps_order_and_closes() {
        let sink = Arc::new(RecordingSink::new());
        assert!(sink.is_empty());
        assert_eq!(send_all(&sink, &[1, 2, 3]), [true, true, true]);
        assert_eq!(sink.events(), [1, 2, 3]);
        assert_eq!(sink.len(), 3);
        sink.close();
        assert!(!sink.is_open());
        assert_eq!(send_all(&sink, &[4]), [false]);
        assert_eq!(sink.take(), [1, 2, 3]);
        assert!(sink.is_empty());
    }

    #[test]
    fn recording_bytes_keeps_batches() {
        let sink = Arc::new(RecordingBytes::new());
        let as_dyn: Arc<dyn ByteSink> = sink.clone();
        assert!(as_dyn.send(b"ab".to_vec()));
        assert!(sink.send(b"c".to_vec()));
        assert_eq!(sink.batches(), [b"ab".to_vec(), b"c".to_vec()]);
        assert_eq!(sink.concat(), b"abc");
        sink.close();
        assert!(!sink.is_open());
        assert!(!as_dyn.send(b"d".to_vec()));
        assert_eq!(sink.take().len(), 2);
        assert!(sink.batches().is_empty());
    }
}
