//! Rate limits for a program's input (`docs/spec/07-toolchain-build-run.md`
//! §7.6.5, `docs/spec/08-security.md` §8.8): at most
//! [`RUN_INPUT_CALLS_PER_SEC`] `run_input` calls and
//! [`RUN_INPUT_BYTES_PER_SEC`] bytes a second per run.
//!
//! Each limit is a token bucket that holds one second's worth and refills
//! continuously, so a burst of a full second's allowance is accepted at once
//! and sustained input is held to the rate. A call is admitted only when
//! both buckets can pay for it, and then pays both; a refused call costs
//! nothing.

use std::time::Instant;

use b2c_ipc::limits::{RUN_INPUT_BYTES_PER_SEC, RUN_INPUT_CALLS_PER_SEC};

/// Credits per token: buckets count in billionths of a token, so a refill
/// for any number of nanoseconds is exact.
const CREDITS_PER_TOKEN: u128 = 1_000_000_000;

/// A token bucket holding at most one second of `per_sec` tokens.
#[derive(Debug, Clone)]
struct TokenBucket {
    per_sec: u128,
    credits: u128,
    last: Option<Instant>,
}

impl TokenBucket {
    /// A full bucket.
    fn full(per_sec: u64) -> Self {
        let per_sec = u128::from(per_sec);
        Self {
            per_sec,
            credits: per_sec * CREDITS_PER_TOKEN,
            last: None,
        }
    }

    fn refill(&mut self, now: Instant) {
        if let Some(last) = self.last {
            // Nanoseconds times tokens a second is credits.
            let earned = now
                .saturating_duration_since(last)
                .as_nanos()
                .saturating_mul(self.per_sec);
            self.credits = self
                .credits
                .saturating_add(earned)
                .min(self.per_sec * CREDITS_PER_TOKEN);
        }
        // A clock that went backwards (impossible for `Instant`) earns
        // nothing rather than being remembered.
        if self.last.is_none_or(|last| now > last) {
            self.last = Some(now);
        }
    }

    fn can_pay(&self, tokens: u64) -> bool {
        self.credits >= u128::from(tokens) * CREDITS_PER_TOKEN
    }

    fn pay(&mut self, tokens: u64) {
        self.credits -= u128::from(tokens) * CREDITS_PER_TOKEN;
    }
}

/// The input limits of one run.
#[derive(Debug, Clone)]
pub(crate) struct InputLimiter {
    calls: TokenBucket,
    bytes: TokenBucket,
}

impl InputLimiter {
    /// The limits of the IPC contract, with full buckets.
    pub(crate) fn new() -> Self {
        Self::with_rates(
            u64::from(RUN_INPUT_CALLS_PER_SEC),
            u64::try_from(RUN_INPUT_BYTES_PER_SEC).unwrap_or(u64::MAX),
        )
    }

    fn with_rates(calls_per_sec: u64, bytes_per_sec: u64) -> Self {
        Self {
            calls: TokenBucket::full(calls_per_sec),
            bytes: TokenBucket::full(bytes_per_sec),
        }
    }

    /// Admits one call of `len` bytes at `now`, or refuses it (costing
    /// nothing) when either limit would be exceeded.
    pub(crate) fn admit(&mut self, len: usize, now: Instant) -> bool {
        let len = u64::try_from(len).unwrap_or(u64::MAX);
        self.calls.refill(now);
        self.bytes.refill(now);
        if !(self.calls.can_pay(1) && self.bytes.can_pay(len)) {
            return false;
        }
        self.calls.pay(1);
        self.bytes.pay(len);
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_second_of_calls_then_the_rate() {
        let start = Instant::now();
        let mut limiter = InputLimiter::new();
        for _ in 0..200 {
            assert!(limiter.admit(1, start));
        }
        assert!(!limiter.admit(1, start), "the 201st call in the same instant");
        // 5 ms later one more call has been earned (200 a second).
        assert!(limiter.admit(1, start + Duration::from_millis(5)));
        assert!(!limiter.admit(1, start + Duration::from_millis(5)));
        // A second later the bucket is full again, but no fuller.
        let later = start + Duration::from_secs(10);
        for _ in 0..200 {
            assert!(limiter.admit(0, later));
        }
        assert!(!limiter.admit(0, later));
    }

    #[test]
    fn a_mebibyte_a_second() {
        let start = Instant::now();
        let mut limiter = InputLimiter::new();
        for _ in 0..16 {
            assert!(limiter.admit(65_536, start));
        }
        assert!(!limiter.admit(1, start), "1 MiB has been sent");
        // Half a second earns half a mebibyte.
        let half = start + Duration::from_millis(500);
        for _ in 0..8 {
            assert!(limiter.admit(65_536, half));
        }
        assert!(!limiter.admit(1, half));
    }

    #[test]
    fn a_refused_call_costs_nothing() {
        let start = Instant::now();
        let mut limiter = InputLimiter::with_rates(10, 100);
        assert!(!limiter.admit(101, start), "more than a second's worth of bytes");
        for _ in 0..10 {
            assert!(limiter.admit(10, start));
        }
        assert!(!limiter.admit(0, start), "no call left");
        let mut limiter = InputLimiter::with_rates(10, 100);
        assert!(limiter.admit(100, start));
        assert!(!limiter.admit(1, start), "no byte left");
        // The refused calls did not use up call tokens: nine are left.
        let later = start + Duration::from_secs(1);
        for _ in 0..10 {
            assert!(limiter.admit(0, later));
        }
    }

    #[test]
    fn time_never_runs_backwards_into_credit() {
        let start = Instant::now() + Duration::from_secs(1);
        let mut limiter = InputLimiter::with_rates(1, 1);
        assert!(limiter.admit(1, start));
        assert!(!limiter.admit(1, start.checked_sub(Duration::from_secs(1)).unwrap()));
        assert!(!limiter.admit(1, start + Duration::from_millis(999)));
        assert!(limiter.admit(1, start + Duration::from_secs(1)));
    }
}
