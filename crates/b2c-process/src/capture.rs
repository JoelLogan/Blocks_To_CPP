//! Draining pipes on helper threads: capped output readers and the stdin
//! writer.

use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Instant;

/// Size of each read from a pipe.
const CHUNK: usize = 64 * 1024;

/// What a reader has collected so far.
#[derive(Debug, Default)]
struct Sink {
    data: Vec<u8>,
    truncated: bool,
    finished: bool,
}

#[derive(Debug, Default)]
struct Shared {
    sink: Mutex<Sink>,
    finished: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Sink> {
        // A panic while holding the lock cannot leave `Sink` inconsistent
        // (every update is a single push or flag), so poisoning is ignored.
        self.sink.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Collected output of one stream.
#[derive(Debug, Default)]
pub(crate) struct Output {
    /// The bytes kept (at most the cap).
    pub(crate) data: Vec<u8>,
    /// Whether bytes beyond the cap were discarded.
    pub(crate) truncated: bool,
}

/// A thread reading one pipe to its end, keeping at most `cap` bytes and
/// discarding the rest so the writer never blocks.
#[derive(Debug)]
pub(crate) struct Reader {
    shared: Arc<Shared>,
}

impl Reader {
    /// Starts reading `source` on a new thread.
    pub(crate) fn spawn<R: Read + Send + 'static>(name: &str, mut source: R, cap: usize) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let worker = Arc::clone(&shared);
        thread::Builder::new()
            .name(format!("b2c-process-{name}"))
            .spawn(move || {
                let mut buffer = vec![0_u8; CHUNK];
                loop {
                    match source.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => {
                            let chunk = buffer.get(..read).unwrap_or_default();
                            let mut sink = worker.lock();
                            let room = cap.saturating_sub(sink.data.len());
                            let kept = chunk.len().min(room);
                            sink.data.extend_from_slice(chunk.get(..kept).unwrap_or_default());
                            if kept < chunk.len() {
                                sink.truncated = true;
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
                worker.lock().finished = true;
                worker.finished.notify_all();
            })?;
        Ok(Self { shared })
    }

    /// Waits until the pipe reaches end-of-file or `deadline` passes, then
    /// returns what was collected. The second value is `false` when the
    /// deadline passed first (a process outside the tree still holds the
    /// pipe open); the thread then keeps draining in the background until
    /// that process exits.
    pub(crate) fn finish(self, deadline: Instant) -> (Output, bool) {
        let mut sink = self.shared.lock();
        while !sink.finished {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            sink = self
                .shared
                .finished
                .wait_timeout(sink, deadline - now)
                .map_or_else(|poisoned| poisoned.into_inner().0, |(guard, _)| guard);
        }
        let complete = sink.finished;
        let output = Output {
            data: std::mem::take(&mut sink.data),
            truncated: sink.truncated,
        };
        (output, complete)
    }
}

/// Writes `bytes` to the child's standard input on a new thread, then closes
/// it. A child that exits without reading everything just ends the write.
pub(crate) fn spawn_writer<W: Write + Send + 'static>(mut sink: W, bytes: Vec<u8>) -> io::Result<()> {
    thread::Builder::new()
        .name(String::from("b2c-process-stdin"))
        .spawn(move || {
            // Errors (usually a broken pipe because the child stopped reading)
            // are expected and harmless: the child simply did not want more.
            let _ = sink.write_all(&bytes);
            let _ = sink.flush();
            drop(sink);
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn keeps_up_to_the_cap_and_flags_the_rest() {
        let input = vec![b'x'; 200_000];
        let reader = Reader::spawn("test", io::Cursor::new(input), 1000).unwrap();
        let (output, complete) = reader.finish(Instant::now() + Duration::from_secs(10));
        assert!(complete);
        assert_eq!(output.data.len(), 1000);
        assert!(output.truncated);
    }

    #[test]
    fn exact_cap_is_not_truncated() {
        let reader = Reader::spawn("test", io::Cursor::new(vec![1_u8; 10]), 10).unwrap();
        let (output, complete) = reader.finish(Instant::now() + Duration::from_secs(10));
        assert!(complete);
        assert_eq!(output.data, vec![1_u8; 10]);
        assert!(!output.truncated);
    }

    #[test]
    fn zero_cap_discards_everything() {
        let reader = Reader::spawn("test", io::Cursor::new(b"abc".to_vec()), 0).unwrap();
        let (output, _) = reader.finish(Instant::now() + Duration::from_secs(10));
        assert!(output.data.is_empty());
        assert!(output.truncated);
    }

    /// A source that never ends, like a pipe held open by an escaped process.
    struct Endless;

    impl Read for Endless {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            thread::sleep(Duration::from_millis(5));
            Err(io::Error::from(io::ErrorKind::Interrupted))
        }
    }

    #[test]
    fn deadline_bounds_the_wait() {
        let reader = Reader::spawn("test", Endless, 10).unwrap();
        let start = Instant::now();
        let (_, complete) = reader.finish(Instant::now() + Duration::from_millis(50));
        assert!(!complete);
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
