//! A circuit breaker with one probe.
//!
//! Closed → (N consecutive failures) → Open → (cooldown) → HalfOpen → (probe succeeds) → Closed, or (probe fails) →
//! Open again. In HalfOpen exactly **one** call is let through; every other caller is refused until it settles.
//!
//! Two details that a simple breaker gets wrong:
//! - A probe whose future is dropped (a cancelled request) counts as a failure, otherwise the breaker would wait for
//!   it forever.
//! - Every state change starts a new *epoch*. A call that began in an earlier epoch (a slow request that started while
//!   the circuit was still closed) cannot close or re-open a circuit that has moved on without it.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug)]
struct Inner {
    state: State,
    consecutive_failures: u32,
    opened_at: Option<Instant>,
    epoch: u64,
}

#[derive(Debug)]
pub struct Breaker {
    threshold: u32,
    cooldown: Duration,
    inner: Mutex<Inner>,
}

impl Breaker {
    #[must_use]
    pub fn new(threshold: u32, cooldown: Duration) -> Arc<Self> {
        Arc::new(Self {
            threshold: threshold.max(1),
            cooldown,
            inner: Mutex::new(Inner {
                state: State::Closed,
                consecutive_failures: 0,
                opened_at: None,
                epoch: 0,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // No arm below can leave `Inner` half-written, so a poisoned lock still guards consistent data.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[must_use]
    pub fn state(&self) -> State {
        self.lock().state
    }

    /// Ask to make a call. `None` means the circuit refuses.
    #[must_use]
    pub fn try_acquire(self: &Arc<Self>) -> Option<Permit> {
        let mut inner = self.lock();
        match inner.state {
            State::Closed => {}
            State::Open
                if inner
                    .opened_at
                    .is_some_and(|at| at.elapsed() >= self.cooldown) =>
            {
                inner.state = State::HalfOpen;
                inner.epoch += 1;
            }
            State::Open | State::HalfOpen => return None,
        }
        let epoch = inner.epoch;
        drop(inner);
        Some(Permit {
            breaker: Arc::clone(self),
            epoch,
            settled: false,
        })
    }

    fn settle(&self, epoch: u64, success: bool) {
        let mut inner = self.lock();
        if inner.epoch != epoch {
            return; // the circuit moved on while this call was in flight
        }
        if success {
            inner.state = State::Closed;
            inner.consecutive_failures = 0;
            inner.opened_at = None;
            inner.epoch += 1;
            return;
        }
        inner.consecutive_failures += 1;
        if inner.state == State::HalfOpen || inner.consecutive_failures >= self.threshold {
            inner.state = State::Open;
            inner.opened_at = Some(Instant::now());
            inner.epoch += 1;
        }
    }
}

/// Permission to make one call. Settle it with [`Permit::success`] or [`Permit::failure`]; dropping it unsettled counts
/// as a failure.
#[derive(Debug)]
pub struct Permit {
    breaker: Arc<Breaker>,
    epoch: u64,
    settled: bool,
}

impl Permit {
    pub fn success(mut self) {
        self.settled = true;
        self.breaker.settle(self.epoch, true);
    }

    pub fn failure(mut self) {
        self.settled = true;
        self.breaker.settle(self.epoch, false);
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        if !self.settled {
            self.breaker.settle(self.epoch, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COOLDOWN: Duration = Duration::from_secs(30);

    fn trip(breaker: &Arc<Breaker>, times: u32) {
        for _ in 0..times {
            breaker
                .try_acquire()
                .expect("closed circuit allows the call")
                .failure();
        }
    }

    #[test]
    fn opens_after_the_threshold_and_not_before() {
        let b = Breaker::new(3, COOLDOWN);
        trip(&b, 2);
        assert_eq!(b.state(), State::Closed);
        trip(&b, 1);
        assert_eq!(b.state(), State::Open);
        assert!(b.try_acquire().is_none());
    }

    #[test]
    fn a_success_resets_the_run_of_failures() {
        let b = Breaker::new(3, COOLDOWN);
        trip(&b, 2);
        b.try_acquire().expect("closed").success();
        trip(&b, 2);
        assert_eq!(
            b.state(),
            State::Closed,
            "two + reset + two is not three in a row"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn lets_exactly_one_probe_through_after_the_cooldown() {
        let b = Breaker::new(1, COOLDOWN);
        trip(&b, 1);
        tokio::time::advance(COOLDOWN).await;

        let probe = b
            .try_acquire()
            .expect("the first caller after the cooldown is the probe");
        assert_eq!(b.state(), State::HalfOpen);
        for _ in 0..50 {
            assert!(
                b.try_acquire().is_none(),
                "no second caller while the probe is in flight"
            );
        }
        probe.success();
        assert_eq!(b.state(), State::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_probe_reopens_for_a_full_cooldown() {
        let b = Breaker::new(1, COOLDOWN);
        trip(&b, 1);
        tokio::time::advance(COOLDOWN).await;
        b.try_acquire().expect("probe").failure();
        assert_eq!(b.state(), State::Open);

        tokio::time::advance(COOLDOWN - Duration::from_secs(1)).await;
        assert!(
            b.try_acquire().is_none(),
            "the clock restarted when the probe failed"
        );
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(b.try_acquire().is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn a_cancelled_probe_does_not_wedge_the_circuit() {
        let b = Breaker::new(1, COOLDOWN);
        trip(&b, 1);
        tokio::time::advance(COOLDOWN).await;
        drop(b.try_acquire().expect("probe")); // the request was cancelled before it settled
        assert_eq!(b.state(), State::Open, "counted as a failure");
        tokio::time::advance(COOLDOWN).await;
        assert!(
            b.try_acquire().is_some(),
            "and the next cooldown lets a new probe through"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_call_from_before_the_trip_cannot_close_the_circuit() {
        let b = Breaker::new(2, COOLDOWN);
        let slow = b.try_acquire().expect("closed"); // starts now, finishes much later
        trip(&b, 2);
        assert_eq!(b.state(), State::Open);
        slow.success();
        assert_eq!(b.state(), State::Open, "a stale success is ignored");
    }
}
