//! Output batching and flood protection for run sessions
//! (`docs/spec/07-toolchain-build-run.md` §7.6.5, `docs/spec/02-architecture.md`
//! §2.5.3).
//!
//! The [`Coalescer`] sits between the thread that reads the program's output
//! and the output channel:
//!
//! * **Batching.** Output is sent in numbered batches (from 1), at most one
//!   every [`FlowLimits::interval`] (16 ms, so at most about 60 a second).
//!   Output that arrives after a quiet spell goes out at once; otherwise it
//!   waits at most one interval.
//! * **Flow control.** The console acknowledges what it has written
//!   ([`Coalescer::acknowledge`], from `run_ack`). While more than
//!   [`FlowLimits::unacked_bytes`] (4 MiB) or [`FlowLimits::unacked_batches`]
//!   batches are unacknowledged, nothing is sent: the console has fallen
//!   behind, and the output that arrives meanwhile is cut to its last
//!   [`FlowLimits::tail_lines`] lines (the console's scrollback). Unsent
//!   output never exceeds [`FlowLimits::tail_bytes`] (8 MiB) in any case.
//!   When the acknowledgements catch up (or the output ends), an
//!   [`Emit::Skipped`] with the number of dropped lines goes first, then the
//!   kept tail. A console that never acknowledges anything therefore costs
//!   bounded memory: the program keeps running, and only its tail is kept.
//! * **Exact counts.** A line is counted as skipped when its line break
//!   (`\n`) was dropped, so the line breaks delivered plus the lines reported
//!   skipped always equal the line breaks the program printed. Only the start
//!   of a single line longer than 8 MiB can be dropped without its line
//!   break; the skipped event then reports the lines whose breaks went (which
//!   may be none).
//!
//! Unsent output is trimmed lazily while it grows (at twice the limits, so the
//! cost stays linear in the output) and exactly before it is sent, so it never
//! holds more than about 16 MiB. The clock is injected so tests control time.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use b2c_ipc::limits::{OUTPUT_BATCH_MS, OUTPUT_UNACKED_MAX};

/// Where the coalescer gets the time from.
pub(crate) trait Clock: Send + 'static {
    /// The current time.
    fn now(&self) -> Instant;
}

/// The system's monotonic clock.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// The most output batches that may be unacknowledged before the console
/// counts as behind (about 16 s of output at 60 batches a second; a console
/// that acknowledges every 100 ms has about seven outstanding).
const MAX_UNACKED_BATCHES: usize = 1024;

/// The most bytes of output kept for a console that has fallen behind.
pub(crate) const MAX_TAIL_BYTES: usize = 8 * 1024 * 1024;

/// The limits a [`Coalescer`] enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FlowLimits {
    /// The shortest time between two batches, and the longest output waits.
    pub(crate) interval: Duration,
    /// Unacknowledged bytes above which the console counts as behind.
    pub(crate) unacked_bytes: u64,
    /// Unacknowledged batches above which the console counts as behind.
    pub(crate) unacked_batches: usize,
    /// The most lines kept for a console that is behind (at least 1).
    pub(crate) tail_lines: u64,
    /// The most bytes kept (at least 1).
    pub(crate) tail_bytes: usize,
}

impl FlowLimits {
    /// The limits of the IPC contract for a console with `scrollback_lines`
    /// lines of scrollback.
    pub(crate) fn for_scrollback(scrollback_lines: u32) -> Self {
        Self {
            interval: Duration::from_millis(OUTPUT_BATCH_MS),
            unacked_bytes: u64::try_from(OUTPUT_UNACKED_MAX).unwrap_or(u64::MAX),
            unacked_batches: MAX_UNACKED_BATCHES,
            tail_lines: u64::from(scrollback_lines.max(1)),
            tail_bytes: MAX_TAIL_BYTES,
        }
    }
}

/// What the coalescer asks its owner to send, in this order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Emit {
    /// An output batch for the output channel.
    Batch {
        /// Its number: 1 for the first batch, then one more each time.
        seq: u64,
        /// The bytes.
        bytes: Vec<u8>,
    },
    /// The `skipped` run event: output was dropped after batch `after_seq`.
    Skipped {
        /// How many line breaks were dropped.
        lines: u64,
        /// How many bytes were dropped (for tests and the log; not sent).
        bytes: u64,
        /// The number of batches sent before the dropped output.
        after_seq: u64,
    },
}

/// Output not sent yet: `bytes[start..]`, with its number of line breaks.
#[derive(Debug, Default)]
struct Pending {
    bytes: Vec<u8>,
    start: usize,
    newlines: u64,
}

impl Pending {
    fn len(&self) -> usize {
        self.bytes.len() - self.start
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn live(&self) -> &[u8] {
        &self.bytes[self.start..]
    }

    fn push(&mut self, data: &[u8]) {
        // Give back the dropped front before growing, so the buffer stays
        // within about twice what it holds.
        if self.start > 0 && self.start >= self.bytes.len() / 2 {
            self.bytes.drain(..self.start);
            self.start = 0;
        }
        self.bytes.extend_from_slice(data);
        self.newlines += count_newlines(data);
    }

    /// Drops the first `count` bytes; returns the line breaks among them.
    fn drop_front(&mut self, count: usize) -> u64 {
        let count = count.min(self.len());
        let dropped = count_newlines(&self.live()[..count]);
        self.start += count;
        self.newlines -= dropped;
        if self.is_empty() {
            self.bytes.clear();
            self.start = 0;
        }
        dropped
    }

    /// Keeps only the last `lines` complete lines (and the line being
    /// written). Returns the line breaks and bytes dropped.
    fn keep_lines(&mut self, lines: u64) -> (u64, usize) {
        let Some(excess) = self.newlines.checked_sub(lines).filter(|&excess| excess > 0) else {
            return (0, 0);
        };
        // The position just after the `excess`-th line break.
        let mut seen = 0;
        let cut = self
            .live()
            .iter()
            .position(|&byte| {
                seen += u64::from(byte == b'\n');
                seen == excess
            })
            .map_or(self.len(), |at| at + 1);
        (self.drop_front(cut), cut)
    }

    /// Keeps at most `max` bytes, starting at a line boundary when there is
    /// one. Returns the line breaks and bytes dropped.
    fn keep_bytes(&mut self, max: usize) -> (u64, usize) {
        let len = self.len();
        if len <= max {
            return (0, 0);
        }
        let at_least = len - max;
        // Cut after the first line break that leaves at most `max` bytes, or
        // inside the line when it is longer than that.
        let cut = self.live()[at_least - 1..]
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(at_least, |offset| at_least + offset);
        (self.drop_front(cut), cut)
    }

    fn take(&mut self) -> Vec<u8> {
        let bytes = if self.start == 0 {
            std::mem::take(&mut self.bytes)
        } else {
            let live = self.live().to_vec();
            self.bytes.clear();
            live
        };
        self.start = 0;
        self.newlines = 0;
        bytes
    }
}

// A plain loop the compiler vectorises; not worth a dependency.
#[allow(clippy::naive_bytecount)]
fn count_newlines(bytes: &[u8]) -> u64 {
    // At most `usize::MAX` line breaks, which always fits in u64.
    bytes.iter().filter(|&&byte| byte == b'\n').count() as u64
}

/// Batches a program's output and protects a slow console from it (see the
/// [module documentation](self)).
#[derive(Debug)]
pub(crate) struct Coalescer<C: Clock> {
    clock: C,
    limits: FlowLimits,
    pending: Pending,
    last_flush: Option<Instant>,
    /// Batches sent so far (the number of the last one).
    seq: u64,
    /// Bytes sent so far.
    sent_bytes: u64,
    /// For each unacknowledged batch: its number and `sent_bytes` after it.
    unacked: VecDeque<(u64, u64)>,
    acked_seq: u64,
    acked_bytes: u64,
    /// Whether output arrived while the console was behind since the last
    /// batch: it is cut to the tail before it goes.
    held: bool,
    /// Line breaks and bytes dropped since the last [`Emit::Skipped`].
    dropped_lines: u64,
    dropped_bytes: u64,
    /// Line breaks dropped in all.
    total_dropped_lines: u64,
}

impl<C: Clock> Coalescer<C> {
    /// A coalescer that has sent nothing yet.
    pub(crate) fn new(clock: C, limits: FlowLimits) -> Self {
        Self {
            clock,
            limits: FlowLimits {
                tail_lines: limits.tail_lines.max(1),
                tail_bytes: limits.tail_bytes.max(1),
                ..limits
            },
            pending: Pending::default(),
            last_flush: None,
            seq: 0,
            sent_bytes: 0,
            unacked: VecDeque::new(),
            acked_seq: 0,
            acked_bytes: 0,
            held: false,
            dropped_lines: 0,
            dropped_bytes: 0,
            total_dropped_lines: 0,
        }
    }

    /// The number of batches sent so far (the `afterSeq` of an event sent
    /// now).
    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }

    /// The line breaks dropped in all.
    pub(crate) fn total_dropped_lines(&self) -> u64 {
        self.total_dropped_lines
    }

    /// Adds output the program printed.
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.pending.push(bytes);
        // The limits are applied lazily here, at twice their size, so each
        // byte is scanned a bounded number of times; `flush` applies them
        // exactly.
        if self.is_behind() {
            self.held = true;
            if self.pending.newlines > self.limits.tail_lines.saturating_mul(2) {
                self.keep_lines();
            }
        }
        if self.pending.len() > self.limits.tail_bytes.saturating_mul(2) {
            self.keep_bytes();
        }
    }

    /// Records that the console has written every batch up to `seq`. Older
    /// or repeated acknowledgements change nothing; batches not sent yet
    /// cannot be acknowledged.
    pub(crate) fn acknowledge(&mut self, seq: u64) {
        let seq = seq.min(self.seq);
        if seq <= self.acked_seq {
            return;
        }
        while let Some(&(batch, sent_after)) = self.unacked.front() {
            if batch > seq {
                break;
            }
            self.acked_bytes = sent_after;
            self.unacked.pop_front();
        }
        self.acked_seq = seq;
    }

    /// Emits what is due now: the next batch when the interval has passed and
    /// the console is not behind, preceded by a skipped event when output
    /// was dropped.
    pub(crate) fn poll(&mut self, out: &mut Vec<Emit>) {
        if !self.has_output() || self.is_behind() {
            return;
        }
        let now = self.clock.now();
        if self.next_flush().is_some_and(|due| now < due) {
            return;
        }
        self.flush(now, out);
    }

    /// Emits everything left, whatever the interval and acknowledgements
    /// (the program's output has ended).
    pub(crate) fn finish(&mut self, out: &mut Vec<Emit>) {
        if self.has_output() {
            let now = self.clock.now();
            self.flush(now, out);
        }
    }

    /// When [`Coalescer::poll`] should next be called if no output arrives
    /// before: `None` when nothing is waiting. While the console is behind,
    /// one interval from now, to look at the acknowledgements again.
    pub(crate) fn wake_at(&self) -> Option<Instant> {
        if !self.has_output() {
            return None;
        }
        let now = self.clock.now();
        if self.is_behind() {
            return Some(now.checked_add(self.limits.interval).unwrap_or(now));
        }
        Some(self.next_flush().unwrap_or(now))
    }

    /// Whether the console has fallen too far behind to be sent more.
    fn is_behind(&self) -> bool {
        self.sent_bytes - self.acked_bytes > self.limits.unacked_bytes
            || self.unacked.len() > self.limits.unacked_batches
    }

    fn has_output(&self) -> bool {
        !self.pending.is_empty() || self.dropped_bytes > 0
    }

    /// The earliest time the next batch may go, or `None` before the first.
    fn next_flush(&self) -> Option<Instant> {
        self.last_flush
            .map(|last| last.checked_add(self.limits.interval).unwrap_or(last))
    }

    fn keep_lines(&mut self) {
        let dropped = self.pending.keep_lines(self.limits.tail_lines);
        self.record_drop(dropped);
    }

    fn keep_bytes(&mut self) {
        let dropped = self.pending.keep_bytes(self.limits.tail_bytes);
        self.record_drop(dropped);
    }

    fn record_drop(&mut self, (lines, bytes): (u64, usize)) {
        self.dropped_lines += lines;
        self.total_dropped_lines += lines;
        self.dropped_bytes += bytes as u64;
    }

    fn flush(&mut self, now: Instant, out: &mut Vec<Emit>) {
        if std::mem::take(&mut self.held) {
            self.keep_lines();
        }
        self.keep_bytes();
        if self.dropped_bytes > 0 {
            out.push(Emit::Skipped {
                lines: self.dropped_lines,
                bytes: self.dropped_bytes,
                after_seq: self.seq,
            });
            self.dropped_lines = 0;
            self.dropped_bytes = 0;
        }
        if !self.pending.is_empty() {
            let bytes = self.pending.take();
            self.seq += 1;
            self.sent_bytes += bytes.len() as u64;
            self.unacked.push_back((self.seq, self.sent_bytes));
            out.push(Emit::Batch { seq: self.seq, bytes });
        }
        self.last_flush = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use proptest::prelude::*;

    use super::*;

    /// A clock the test moves by hand.
    #[derive(Debug, Clone)]
    struct ManualClock(Arc<Mutex<Instant>>);

    impl ManualClock {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Instant::now())))
        }

        fn advance(&self, by: Duration) {
            let mut now = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            *now += by;
        }

        fn set(&self, at: Instant) {
            *self.0.lock().unwrap_or_else(PoisonError::into_inner) = at;
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> Instant {
            *self.0.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    const MS: Duration = Duration::from_millis(1);

    fn limits(unacked_bytes: u64, tail_lines: u64, tail_bytes: usize) -> FlowLimits {
        FlowLimits {
            interval: 16 * MS,
            unacked_bytes,
            unacked_batches: 1024,
            tail_lines,
            tail_bytes,
        }
    }

    fn batches(emits: &[Emit]) -> Vec<&[u8]> {
        emits
            .iter()
            .filter_map(|emit| match emit {
                Emit::Batch { bytes, .. } => Some(bytes.as_slice()),
                Emit::Skipped { .. } => None,
            })
            .collect()
    }

    fn skipped_lines(emits: &[Emit]) -> u64 {
        emits
            .iter()
            .map(|emit| match emit {
                Emit::Skipped { lines, .. } => *lines,
                Emit::Batch { .. } => 0,
            })
            .sum()
    }

    fn delivered_lines(emits: &[Emit]) -> u64 {
        batches(emits).iter().map(|b| count_newlines(b)).sum()
    }

    /// Checks the numbering: batches 1, 2, 3, … and each skipped event's
    /// `after_seq` equal to the batches before it.
    fn check_numbering(emits: &[Emit]) {
        let mut seq = 0;
        for emit in emits {
            match emit {
                Emit::Batch { seq: number, .. } => {
                    seq += 1;
                    assert_eq!(*number, seq);
                }
                Emit::Skipped { after_seq, .. } => assert_eq!(*after_seq, seq),
            }
        }
    }

    #[test]
    fn the_contract_limits() {
        let limits = FlowLimits::for_scrollback(10_000);
        assert_eq!(limits.interval, 16 * MS);
        assert_eq!(limits.unacked_bytes, 4 * 1024 * 1024);
        assert_eq!(limits.tail_lines, 10_000);
        assert_eq!(limits.tail_bytes, 8 * 1024 * 1024);
        assert_eq!(FlowLimits::for_scrollback(0).tail_lines, 1);
    }

    #[test]
    fn batches_go_at_most_every_interval_and_output_waits_at_most_one() {
        let clock = ManualClock::new();
        let mut coalescer = Coalescer::new(clock.clone(), limits(u64::MAX, 100, 1 << 20));
        let mut out = Vec::new();
        assert_eq!(coalescer.wake_at(), None, "nothing to send");

        // The first output goes at once.
        coalescer.push(b"a");
        assert_eq!(coalescer.wake_at(), Some(clock.now()));
        coalescer.poll(&mut out);
        assert_eq!(batches(&out), [b"a"]);

        // Output 1 ms later waits until 16 ms after the last batch.
        clock.advance(MS);
        coalescer.push(b"b");
        let due = coalescer.wake_at().unwrap();
        assert_eq!(due - clock.now(), 15 * MS);
        coalescer.poll(&mut out);
        assert_eq!(out.len(), 1, "too early");
        clock.advance(14 * MS);
        coalescer.push(b"c");
        coalescer.push(b"");
        coalescer.poll(&mut out);
        assert_eq!(out.len(), 1, "still too early");
        clock.set(due);
        coalescer.poll(&mut out);
        assert_eq!(batches(&out), [&b"a"[..], b"bc"]);
        assert_eq!(coalescer.wake_at(), None);

        // After a quiet spell, output goes at once again.
        clock.advance(100 * MS);
        coalescer.push(b"d");
        coalescer.poll(&mut out);
        assert_eq!(batches(&out).len(), 3);
        check_numbering(&out);
    }

    /// Drives a coalescer the way the session's pump thread does: output
    /// arrives at the given times (after the start), the pump polls at each
    /// arrival and at every time the coalescer asks for, the console
    /// acknowledges everything sent every `ack_every`, and the output ends
    /// with the last arrival. Returns the start and every emit with its time.
    fn pump(
        arrivals: &[(Duration, Vec<u8>)],
        limits: FlowLimits,
        ack_every: Option<Duration>,
    ) -> (Instant, Vec<(Instant, Emit)>) {
        let clock = ManualClock::new();
        let start = clock.now();
        let mut coalescer = Coalescer::new(clock.clone(), limits);
        let mut log = Vec::new();
        let mut out = Vec::new();
        let mut next_ack = ack_every.map(|every| start + every);
        let mut arrivals = arrivals.iter().peekable();
        while let Some(&(at, _)) = arrivals.peek() {
            let next = [Some(start + *at), coalescer.wake_at(), next_ack]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(start);
            clock.set(next.max(clock.now()));
            if let (Some(due), Some(every)) = (next_ack, ack_every)
                && due <= clock.now()
            {
                coalescer.acknowledge(coalescer.seq());
                next_ack = Some(clock.now() + every);
            }
            while let Some((_, bytes)) = arrivals.next_if(|(at, _)| start + *at <= clock.now()) {
                coalescer.push(bytes);
            }
            coalescer.poll(&mut out);
            log.extend(out.drain(..).map(|emit| (clock.now(), emit)));
        }
        coalescer.finish(&mut out);
        log.extend(out.drain(..).map(|emit| (clock.now(), emit)));
        (start, log)
    }

    #[test]
    fn a_steady_stream_is_sent_every_interval() {
        // A line every 3 ms for a second.
        let arrivals: Vec<(Duration, Vec<u8>)> = (0..333).map(|i| (3 * MS * i, b"x\n".to_vec())).collect();
        let (_, log) = pump(&arrivals, limits(u64::MAX, 100, 1 << 20), None);
        let times: Vec<Instant> = log.iter().map(|(at, _)| *at).collect();
        for pair in times[..times.len() - 1].windows(2) {
            assert!(
                pair[1] - pair[0] == 16 * MS,
                "batches every 16 ms while output flows"
            );
        }
        let emits: Vec<Emit> = log.into_iter().map(|(_, emit)| emit).collect();
        assert_eq!(delivered_lines(&emits), 333);
        assert_eq!(skipped_lines(&emits), 0);
        check_numbering(&emits);
    }

    #[test]
    fn dropped_lines_are_counted_exactly_without_acknowledgements() {
        // 4 KiB unacknowledged at most, 100 lines of tail: 100,000 lines of
        // output, never acknowledged.
        let lines: Vec<u8> = (0..100_000)
            .flat_map(|i| format!("line {i}\n").into_bytes())
            .collect();
        let clock = ManualClock::new();
        let mut coalescer = Coalescer::new(clock.clone(), limits(4096, 100, 1 << 20));
        let mut out = Vec::new();
        for chunk in lines.chunks(1000) {
            coalescer.push(chunk);
            coalescer.poll(&mut out);
            clock.advance(MS);
            assert!(coalescer.pending.len() <= 20_000, "unsent output is bounded");
        }
        coalescer.finish(&mut out);
        check_numbering(&out);
        assert_eq!(delivered_lines(&out) + skipped_lines(&out), 100_000);
        assert_eq!(coalescer.total_dropped_lines(), skipped_lines(&out));
        // Two batches went before the console counted as behind; then one
        // skipped event and exactly the last 100 lines.
        assert_eq!(out.len(), 4);
        assert!(matches!(out[2], Emit::Skipped { after_seq: 2, .. }));
        let last = *batches(&out).last().unwrap();
        assert_eq!(count_newlines(last), 100);
        assert!(last.starts_with(b"line 99900\n") && last.ends_with(b"line 99999\n"));
    }

    #[test]
    fn acknowledgements_resume_output_with_a_skipped_event_first() {
        let clock = ManualClock::new();
        let mut coalescer = Coalescer::new(clock.clone(), limits(10, 2, 1 << 20));
        let mut out = Vec::new();
        coalescer.push(b"first output\n");
        coalescer.poll(&mut out);
        assert_eq!(coalescer.seq(), 1);
        // 13 bytes unacknowledged > 10: behind.
        clock.advance(20 * MS);
        coalescer.push(b"a\nb\nc\nd\ne\nf\ng\n");
        coalescer.poll(&mut out);
        assert_eq!(out.len(), 1, "nothing is sent while behind");
        assert_eq!(
            coalescer.wake_at(),
            Some(clock.now() + 16 * MS),
            "polls for acknowledgements"
        );

        coalescer.acknowledge(1);
        coalescer.poll(&mut out);
        assert_eq!(
            out[1..],
            [
                Emit::Skipped {
                    lines: 5,
                    bytes: 10,
                    after_seq: 1
                },
                Emit::Batch {
                    seq: 2,
                    bytes: b"f\ng\n".to_vec()
                }
            ]
        );
        // Acknowledging again or out of order changes nothing; batches never
        // sent cannot be acknowledged.
        coalescer.acknowledge(0);
        coalescer.acknowledge(99);
        assert_eq!(coalescer.acked_seq, 2);
        clock.advance(20 * MS);
        coalescer.push(b"h\n");
        coalescer.poll(&mut out);
        assert_eq!(
            out.last(),
            Some(&Emit::Batch {
                seq: 3,
                bytes: b"h\n".to_vec()
            })
        );
    }

    #[test]
    fn a_line_longer_than_the_byte_limit_is_cut() {
        let clock = ManualClock::new();
        let mut coalescer = Coalescer::new(clock.clone(), limits(0, 10, 8));
        let mut out = Vec::new();
        coalescer.push(b"x\n");
        coalescer.poll(&mut out); // now behind (2 > 0)
        coalescer.push(b"0123456789abcdefghij");
        coalescer.acknowledge(1);
        clock.advance(20 * MS);
        coalescer.poll(&mut out);
        assert_eq!(
            out[1..],
            [
                Emit::Skipped {
                    lines: 0,
                    bytes: 12,
                    after_seq: 1
                },
                Emit::Batch {
                    seq: 2,
                    bytes: b"cdefghij".to_vec()
                }
            ]
        );
        // With a line break past the cut, the tail starts after it.
        coalescer.acknowledge(2);
        clock.advance(20 * MS);
        coalescer.push(b"0123\n56789\nab");
        coalescer.poll(&mut out);
        assert_eq!(
            out[3..],
            [
                Emit::Skipped {
                    lines: 1,
                    bytes: 5,
                    after_seq: 2
                },
                Emit::Batch {
                    seq: 3,
                    bytes: b"56789\nab".to_vec()
                }
            ]
        );
    }

    #[test]
    fn too_many_unacknowledged_batches_count_as_behind() {
        let clock = ManualClock::new();
        let mut coalescer = Coalescer::new(
            clock.clone(),
            FlowLimits {
                unacked_batches: 3,
                ..limits(u64::MAX, 5, 1 << 20)
            },
        );
        let mut out = Vec::new();
        for _ in 0..10 {
            coalescer.push(b"tick\n");
            coalescer.poll(&mut out);
            clock.advance(20 * MS);
        }
        assert_eq!(coalescer.seq(), 4, "the fourth batch made it behind");
        coalescer.acknowledge(4);
        coalescer.poll(&mut out);
        assert_eq!(coalescer.seq(), 5);
        assert_eq!(skipped_lines(&out), 1, "6 lines waited; the last 5 were kept");
        assert_eq!(delivered_lines(&out), 9);
        check_numbering(&out);
    }

    #[test]
    fn finish_sends_everything_whatever_the_state() {
        let clock = ManualClock::new();
        let mut coalescer = Coalescer::new(clock.clone(), limits(0, 3, 1 << 20));
        let mut out = Vec::new();
        coalescer.push(b"1\n");
        coalescer.poll(&mut out);
        coalescer.push(b"2\n3\n4\n5\n6\n7\n8");
        coalescer.finish(&mut out);
        assert_eq!(
            out[1..],
            [
                Emit::Skipped {
                    lines: 3,
                    bytes: 6,
                    after_seq: 1
                },
                Emit::Batch {
                    seq: 2,
                    bytes: b"5\n6\n7\n8".to_vec()
                }
            ]
        );
        let mut more = Vec::new();
        coalescer.finish(&mut more);
        assert!(more.is_empty());
        assert_eq!(coalescer.wake_at(), None);
    }

    #[test]
    fn unsent_output_stays_bounded_while_the_console_is_behind() {
        let clock = ManualClock::new();
        let mut coalescer = Coalescer::new(clock.clone(), limits(0, 1_000, 1024));
        let mut out = Vec::new();
        coalescer.push(b"x");
        coalescer.poll(&mut out);
        for _ in 0..10_000 {
            coalescer.push(&[b'y'; 100]);
            assert!(coalescer.pending.len() <= 2 * 1024 + 100);
            assert!(
                coalescer.pending.bytes.len() <= 2 * (2 * 1024 + 100),
                "the dropped front is given back"
            );
        }
    }

    /// Checks that `emits` is `input` with some parts left out, each marked
    /// by a skipped event with the number of line breaks and bytes it held.
    fn check_faithful(input: &[u8], emits: &[Emit]) -> Result<(), TestCaseError> {
        let mut cursor = 0;
        for emit in emits {
            match emit {
                Emit::Skipped { lines, bytes, .. } => {
                    let end = cursor + usize::try_from(*bytes).unwrap();
                    prop_assert!(end <= input.len(), "more was skipped than printed");
                    prop_assert_eq!(count_newlines(&input[cursor..end]), *lines);
                    cursor = end;
                }
                Emit::Batch { bytes, .. } => {
                    prop_assert!(
                        input[cursor..].starts_with(bytes),
                        "a batch is not the next output"
                    );
                    cursor += bytes.len();
                }
            }
        }
        prop_assert_eq!(cursor, input.len(), "output was lost without a skipped event");
        Ok(())
    }

    fn arrivals(chunks: Vec<(u64, Vec<u8>)>) -> Vec<(Duration, Vec<u8>)> {
        let mut at = Duration::ZERO;
        chunks
            .into_iter()
            .map(|(gap, bytes)| {
                at += Duration::from_millis(gap);
                (at, bytes)
            })
            .collect()
    }

    proptest! {
        #[test]
        fn output_is_delivered_or_counted_as_skipped(
            chunks in proptest::collection::vec(
                (0u64..40, proptest::collection::vec(prop_oneof![Just(b'\n'), Just(b'x'), Just(b'y')], 0..64)),
                0..60,
            ),
            unacked in 1u64..200,
            tail_lines in 1u64..8,
            tail_bytes in 1usize..64,
            ack_every in proptest::option::of(1u64..80),
        ) {
            let arrivals = arrivals(chunks);
            let input: Vec<u8> = arrivals.iter().flat_map(|(_, bytes)| bytes.clone()).collect();
            let (_, log) = pump(
                &arrivals,
                limits(unacked, tail_lines, tail_bytes),
                ack_every.map(Duration::from_millis),
            );
            let emits: Vec<Emit> = log.iter().map(|(_, emit)| emit.clone()).collect();
            check_numbering(&emits);
            prop_assert_eq!(delivered_lines(&emits) + skipped_lines(&emits), count_newlines(&input));
            check_faithful(&input, &emits)?;
            let batch_times: Vec<Instant> = log
                .iter()
                .filter(|(_, emit)| matches!(emit, Emit::Batch { .. }))
                .map(|(at, _)| *at)
                .collect();
            // Only the final batch, sent when the output ends, may follow the
            // one before it early.
            for pair in batch_times[..batch_times.len().saturating_sub(1)].windows(2) {
                prop_assert!(pair[1] - pair[0] >= 16 * MS);
            }
        }

        #[test]
        fn with_prompt_acknowledgements_nothing_is_dropped_or_late(
            chunks in proptest::collection::vec((0u64..40, proptest::collection::vec(any::<u8>(), 0..64)), 0..60),
        ) {
            let arrivals = arrivals(chunks);
            let input: Vec<u8> = arrivals.iter().flat_map(|(_, bytes)| bytes.clone()).collect();
            let (start, log) = pump(&arrivals, limits(1 << 20, 1, 1 << 20), Some(MS));
            let emits: Vec<Emit> = log.iter().map(|(_, emit)| emit.clone()).collect();
            prop_assert_eq!(skipped_lines(&emits), 0);
            prop_assert_eq!(batches(&emits).concat(), input);
            // Every batch goes within one interval of the oldest byte in it.
            let mut chunk_starts = Vec::new();
            let mut offset = 0;
            for (at, bytes) in &arrivals {
                if !bytes.is_empty() {
                    chunk_starts.push((offset, start + *at));
                }
                offset += bytes.len();
            }
            let mut consumed = 0;
            for (sent_at, emit) in &log {
                if let Emit::Batch { bytes, .. } = emit {
                    let oldest = chunk_starts
                        .iter()
                        .rev()
                        .find(|(offset, _)| *offset <= consumed)
                        .map(|(_, at)| *at);
                    prop_assert!(oldest.is_some_and(|oldest| *sent_at - oldest <= 16 * MS));
                    consumed += bytes.len();
                }
            }
        }
    }
}
