//! `fleet-kernel` — Hexagonal Architecture Microkernel for AI-Fleet.
//!
//! Provides pure Dependency-Inversion Ports (`traits`), a SHA-256 hash-chained
//! typed `DomainEventBus`, `CircuitBreaker` + `Bulkhead` resilience primitives,
//! a distributed `WorkStealingBroker`, and the `PluginInterceptor` middleware contract.

pub mod claims;
pub mod events;
pub mod pipeline;
pub mod ports;
pub mod resilience;

pub use claims::{ClaimSnapshot, WorkStealingBroker};
pub use events::{DomainEventBus, DomainEventPayload, KernelEvent};
pub use pipeline::{HookPhase, InterceptContext, InterceptOutcome, PluginInterceptor};
pub use ports::*;
pub use resilience::{backoff_with_jitter, Bulkhead, CircuitBreaker, CircuitBreakerConfig, CircuitState};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn event_bus_hash_chain_and_replay() {
        let bus = DomainEventBus::new(64);
        bus.publish("corr-1", Some("t1".into()), DomainEventPayload::TaskQueued { title: "A".into(), complexity: "deep".into() });
        bus.publish("corr-1", Some("t1".into()), DomainEventPayload::LeaseClaimed { worker_id: "w1".into(), epoch: 1 });
        assert_eq!(bus.verify_ring_integrity(), Ok(2));
        assert_eq!(bus.replay_since(2).len(), 1);
    }

    #[test]
    fn circuit_breaker_state_machine_transitions() {
        let cb = CircuitBreaker::new(
            "claude",
            CircuitBreakerConfig {
                failure_threshold: 2,
                half_open_success_threshold: 1,
                open_duration: Duration::from_millis(25),
            },
        );
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
        assert_eq!(cb.record_failure(), None);
        assert_eq!(cb.record_failure(), Some((CircuitState::Closed, CircuitState::Open)));
        assert!(!cb.allow_request(), "open breaker must reject calls");

        std::thread::sleep(Duration::from_millis(35));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        assert!(cb.allow_request(), "first half-open probe allowed");
        assert!(!cb.allow_request(), "concurrent second probe blocked while first is in flight");
        assert_eq!(cb.record_success(), Some((CircuitState::HalfOpen, CircuitState::Closed)));
        assert!(cb.allow_request());
    }

    #[tokio::test]
    async fn bulkhead_enforces_concurrency_cap() {
        let bh = Bulkhead::new("codex", 1);
        let _p1 = bh.acquire_timeout(Duration::from_millis(50)).await.unwrap();
        assert_eq!(bh.available_permits(), 0);
        let second = bh.acquire_timeout(Duration::from_millis(25)).await;
        assert!(second.is_err());
    }

    #[test]
    fn work_stealing_and_stalled_lease_reclamation() {
        let broker = WorkStealingBroker::new(Duration::from_millis(20));
        broker.enqueue_local("worker-a", "task-1");
        broker.enqueue_local("worker-a", "task-2");

        // worker-b has empty queue -> steals task-1 from worker-a!
        let (tid, ep1, stolen) = broker.next_or_steal("worker-b").unwrap();
        assert_eq!(tid, "task-1");
        assert_eq!(stolen.as_deref(), Some("worker-a"));
        assert!(broker.heartbeat("task-1", ep1, 50));

        // Let lease expire and reclaim onto worker-c
        std::thread::sleep(Duration::from_millis(30));
        let reclaimed = broker.reclaim_stalled("worker-c");
        assert_eq!(reclaimed.len(), 1);
        assert!(!broker.complete("task-1", ep1), "fenced old epoch must fail to complete");

        let (tid2, ep2, _) = broker.next_or_steal("worker-c").unwrap();
        assert_eq!(tid2, "task-1");
        assert!(ep2 > ep1);
        assert!(broker.complete("task-1", ep2));
    }
}
