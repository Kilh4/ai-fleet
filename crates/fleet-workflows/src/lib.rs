//! `fleet-workflows` — 3-Layer Hexagonal Workflow, Saga, SPARC & TDD Engine:
//! - `domain`: SPARC stages & gate evidence, TDD phases, Saga outcomes, DaemonWorkerSpec.
//! - `application`: Guarded `SparcStateMachine`, `TddCycleMachine`, LIFO `execute_saga` coordinator.
//! - `infrastructure`: 12 background daemon workers and interval tick evaluator.

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::{
    advance_sparc_phase, create_pair_session, execute_saga, SparcStateMachine, TddCycleMachine,
};
pub use domain::{
    DaemonWorkerSpec, PairProgrammingSession, SagaOutcome, SparcGateEvidence,
    SparcPipelineExecution, SparcStage, TddPhase, SPARC_PHASES,
};
pub use infrastructure::{built_in_12_daemon_workers, due_daemon_workers};

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn saga_rolls_back_completed_steps_in_reverse_lifo_order_on_failure() {
        let log: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let steps: Vec<(&str, Box<dyn FnMut() -> Result<(), String>>, Box<dyn FnMut()>)> = vec![
            (
                "create-worktree",
                Box::new(|| { log.borrow_mut().push("do:wt".into()); Ok(()) }),
                Box::new(|| log.borrow_mut().push("undo:wt".into())),
            ),
            (
                "apply-migration",
                Box::new(|| { log.borrow_mut().push("do:mig".into()); Ok(()) }),
                Box::new(|| log.borrow_mut().push("undo:mig".into())),
            ),
            (
                "merge-main",
                Box::new(|| Err("merge conflict".into())),
                Box::new(|| log.borrow_mut().push("undo:merge".into())),
            ),
        ];

        let out = execute_saga(steps);
        assert_eq!(
            out,
            SagaOutcome::Compensated {
                failed_step: "merge-main".into(),
                error: "merge conflict".into(),
                rolled_back_lifo: vec!["apply-migration".into(), "create-worktree".into()],
            }
        );
        assert_eq!(*log.borrow(), vec!["do:wt", "do:mig", "undo:mig", "undo:wt"]);
    }

    #[test]
    fn sparc_and_tdd_machines_reject_invalid_transitions() {
        let mut sparc = SparcStateMachine::default();
        let mut ev = SparcGateEvidence::default();
        assert!(sparc.try_advance(&ev).is_err());
        ev.has_acceptance_criteria = true;
        assert_eq!(sparc.try_advance(&ev).unwrap(), SparcStage::Pseudocode);

        let mut tdd = TddCycleMachine::default();
        assert!(tdd.record_test_run(true).is_err(), "Red phase requires failing test first");
        assert_eq!(tdd.record_test_run(false).unwrap(), TddPhase::MakeTestPass);
        assert_eq!(tdd.record_test_run(true).unwrap(), TddPhase::RefactorClean);
        assert_eq!(tdd.record_test_run(true).unwrap(), TddPhase::WriteFailingTest);
        assert_eq!(tdd.cycles_completed, 1);
    }
}
