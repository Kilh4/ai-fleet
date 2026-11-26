//! Application layer for `fleet-workflows`:
//! - `SparcStateMachine`: Guarded transitions enforcing per-stage gate evidence.
//! - `TddCycleMachine`: Strict `Red -> Green -> Refactor` enforcement.
//! - `execute_saga`: Transactional Saga runner with LIFO compensating rollbacks.

use crate::domain::*;
use chrono::Utc;

pub fn advance_sparc_phase(current: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    let idx = SPARC_PHASES
        .iter()
        .position(|(name, _, _, _)| name.eq_ignore_ascii_case(current))
        .map(|i| (i + 1).min(SPARC_PHASES.len() - 1))
        .unwrap_or(0);
    SPARC_PHASES[idx]
}

#[derive(Debug, Clone)]
pub struct SparcStateMachine {
    pub stage: SparcStage,
}

impl Default for SparcStateMachine {
    fn default() -> Self {
        SparcStateMachine { stage: SparcStage::Spec }
    }
}

impl SparcStateMachine {
    pub fn try_advance(&mut self, ev: &SparcGateEvidence) -> Result<SparcStage, &'static str> {
        let next = match self.stage {
            SparcStage::Spec => {
                if !ev.has_acceptance_criteria {
                    return Err("Spec gate blocked: acceptance criteria not defined");
                }
                SparcStage::Pseudocode
            }
            SparcStage::Pseudocode => {
                if !ev.has_domain_invariants {
                    return Err("Pseudocode gate blocked: domain invariants missing");
                }
                SparcStage::Architecture
            }
            SparcStage::Architecture => {
                if !ev.architecture_contracts_clean {
                    return Err("Architecture gate blocked: contract/RLS boundaries not verified");
                }
                SparcStage::Refinement
            }
            SparcStage::Refinement => {
                if !ev.verification_tests_green {
                    return Err("Refinement gate blocked: verification test suite is not green");
                }
                SparcStage::Completion
            }
            SparcStage::Completion => {
                if !ev.verification_tests_green || !ev.non_author_review_approved {
                    return Err("Completion gate blocked: requires green tests AND independent non-author approval");
                }
                SparcStage::Done
            }
            SparcStage::Done => SparcStage::Done,
        };
        self.stage = next;
        Ok(next)
    }
}

#[derive(Debug, Clone)]
pub struct TddCycleMachine {
    pub phase: TddPhase,
    pub cycles_completed: u32,
}

impl Default for TddCycleMachine {
    fn default() -> Self {
        TddCycleMachine { phase: TddPhase::WriteFailingTest, cycles_completed: 0 }
    }
}

impl TddCycleMachine {
    /// Observe a test run result (`tests_passed`) and advance iff TDD discipline holds.
    pub fn record_test_run(&mut self, tests_passed: bool) -> Result<TddPhase, &'static str> {
        match self.phase {
            TddPhase::WriteFailingTest => {
                if tests_passed {
                    return Err("TDD Red violation: new test passed immediately before implementation");
                }
                self.phase = TddPhase::MakeTestPass;
            }
            TddPhase::MakeTestPass => {
                if !tests_passed {
                    return Err("TDD Green pending: test suite still failing");
                }
                self.phase = TddPhase::RefactorClean;
            }
            TddPhase::RefactorClean => {
                if !tests_passed {
                    return Err("TDD Refactor regression: refactoring broke tests");
                }
                self.cycles_completed += 1;
                self.phase = TddPhase::WriteFailingTest;
            }
        }
        Ok(self.phase)
    }
}

/// Execute a sequence of `(step_name, action_fn, compensate_fn)` steps as a Saga.
/// If step `i` fails, runs `compensate_fn` for `i-1 ..= 0` in strict reverse (LIFO) order!
pub fn execute_saga<A, C>(steps: Vec<(&str, A, C)>) -> SagaOutcome
where
    A: FnMut() -> Result<(), String>,
    C: FnMut(),
{
    let mut completed_names: Vec<String> = Vec::new();
    let mut compensations: Vec<(String, C)> = Vec::new();

    for (name, mut action, comp) in steps {
        match action() {
            Ok(()) => {
                completed_names.push(name.to_string());
                compensations.push((name.to_string(), comp));
            }
            Err(err) => {
                let mut rolled_back = Vec::with_capacity(compensations.len());
                while let Some((comp_name, mut comp_fn)) = compensations.pop() {
                    comp_fn();
                    rolled_back.push(comp_name);
                }
                return SagaOutcome::Compensated {
                    failed_step: name.to_string(),
                    error: err,
                    rolled_back_lifo: rolled_back,
                };
            }
        }
    }

    SagaOutcome::Committed { completed_steps: completed_names }
}

pub fn create_pair_session(objective: &str) -> PairProgrammingSession {
    PairProgrammingSession {
        session_id: format!("pair-{}", Utc::now().timestamp()),
        driver_provider: "codex".into(),
        driver_model: "gpt-6.2-astra".into(),
        driver_effort: "high".into(),
        navigator_provider: "antigravity".into(),
        navigator_model: "argon".into(),
        navigator_effort: "xhigh".into(),
        objective: objective.to_string(),
        swaps: 0,
    }
}
