//! Enterprise Resilience Primitives:
//! - [`CircuitBreaker`]: 3-state (`Closed -> Open -> HalfOpen`) fault isolator
//!   with sliding failure threshold, recovery timeout, and half-open probe cap.
//! - [`Bulkhead`]: Semaphore-backed concurrency isolator preventing one slow
//!   provider or subsystem from exhausting global worker threads.
//! - [`ExponentialBackoff`]: Deterministic full-jitter backoff calculator.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitState {
    pub fn as_str(self) -> &'static str {
        match self {
            CircuitState::Closed => "closed",
            CircuitState::Open => "open",
            CircuitState::HalfOpen => "half_open",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub half_open_success_threshold: u32,
    pub open_duration: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        CircuitBreakerConfig {
            failure_threshold: 3,
            half_open_success_threshold: 1,
            open_duration: Duration::from_secs(60),
        }
    }
}

struct BreakerInner {
    state: CircuitState,
    consecutive_failures: u32,
    half_open_successes: u32,
    opened_at: Option<Instant>,
    half_open_probe_in_flight: bool,
}

#[derive(Clone)]
pub struct CircuitBreaker {
    name: String,
    cfg: CircuitBreakerConfig,
    inner: Arc<Mutex<BreakerInner>>,
}

impl CircuitBreaker {
    pub fn new(name: impl Into<String>, cfg: CircuitBreakerConfig) -> Self {
        CircuitBreaker {
            name: name.into(),
            cfg,
            inner: Arc::new(Mutex::new(BreakerInner {
                state: CircuitState::Closed,
                consecutive_failures: 0,
                half_open_successes: 0,
                opened_at: None,
                half_open_probe_in_flight: false,
            })),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn state(&self) -> CircuitState {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.maybe_transition_to_half_open(&mut g);
        g.state
    }

    fn maybe_transition_to_half_open(&self, g: &mut BreakerInner) {
        if g.state == CircuitState::Open {
            if let Some(t) = g.opened_at {
                if t.elapsed() >= self.cfg.open_duration {
                    g.state = CircuitState::HalfOpen;
                    g.half_open_successes = 0;
                    g.half_open_probe_in_flight = false;
                }
            }
        }
    }

    /// Check whether a call is allowed right now. In `HalfOpen`, only one probe
    /// call may run concurrently to test recovery safely.
    pub fn allow_request(&self) -> bool {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.maybe_transition_to_half_open(&mut g);
        match g.state {
            CircuitState::Closed => true,
            CircuitState::Open => false,
            CircuitState::HalfOpen => {
                if g.half_open_probe_in_flight {
                    false
                } else {
                    g.half_open_probe_in_flight = true;
                    true
                }
            }
        }
    }

    /// Record a successful call; returns `Some((old, new))` if circuit state transitioned.
    pub fn record_success(&self) -> Option<(CircuitState, CircuitState)> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let old = g.state;
        g.consecutive_failures = 0;
        g.half_open_probe_in_flight = false;
        if g.state == CircuitState::HalfOpen {
            g.half_open_successes += 1;
            if g.half_open_successes >= self.cfg.half_open_success_threshold.max(1) {
                g.state = CircuitState::Closed;
                g.opened_at = None;
            }
        }
        (old != g.state).then_some((old, g.state))
    }

    /// Record a failing call; returns `Some((old, new))` if circuit state transitioned.
    pub fn record_failure(&self) -> Option<(CircuitState, CircuitState)> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let old = g.state;
        g.consecutive_failures += 1;
        g.half_open_probe_in_flight = false;
        if g.state == CircuitState::HalfOpen || g.consecutive_failures >= self.cfg.failure_threshold.max(1) {
            g.state = CircuitState::Open;
            g.opened_at = Some(Instant::now());
            g.half_open_successes = 0;
        }
        (old != g.state).then_some((old, g.state))
    }
}

/// Concurrency Bulkhead isolating a resource pool with bounded wait timeout.
#[derive(Clone)]
pub struct Bulkhead {
    name: String,
    max_concurrent: usize,
    sem: Arc<Semaphore>,
}

impl Bulkhead {
    pub fn new(name: impl Into<String>, max_concurrent: usize) -> Self {
        let cap = max_concurrent.max(1);
        Bulkhead { name: name.into(), max_concurrent: cap, sem: Arc::new(Semaphore::new(cap)) }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    pub fn available_permits(&self) -> usize {
        self.sem.available_permits()
    }

    pub async fn acquire_timeout(&self, wait: Duration) -> Result<OwnedSemaphorePermit, String> {
        tokio::time::timeout(wait, self.sem.clone().acquire_owned())
            .await
            .map_err(|_| format!("bulkhead `{}` saturated (max={})", self.name, self.max_concurrent))?
            .map_err(|_| format!("bulkhead `{}` closed", self.name))
    }
}

/// Deterministic decorrelated jitter backoff (`base * 2^attempt`, capped at `max_delay`).
pub fn backoff_with_jitter(base: Duration, max_delay: Duration, attempt: u32, jitter_seed: u64) -> Duration {
    let shift = attempt.min(16);
    let raw_ms = (base.as_millis() as u64).saturating_mul(1u64 << shift);
    let capped_ms = raw_ms.min(max_delay.as_millis() as u64).max(1);
    // SplitMix64 mixer for deterministic reproducible jitter in [50%, 100%] of capped_ms
    let mut z = jitter_seed.wrapping_add((attempt as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let half = capped_ms / 2;
    let jitter = if half == 0 { 0 } else { z % (half + 1) };
    Duration::from_millis(half + jitter)
}
