//! Runtime invariants over a recorded log.
//!
//! An unperturbed run emits a compliant log by construction, so a checker that
//! only ever agrees with it proves nothing. The credibility comes from
//! [`crate::fault`]: each injected fault must drop exactly the invariant it
//! corresponds to and leave the others standing.

use crate::{
    log::{EventKind, EventLog},
    IntentId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invariant {
    /// Every event carries a trace id.
    TraceId,
    /// Every intent reaches a terminal event.
    TerminalReconstructable,
    /// Every refusal names the policy it violated.
    PolicyIdOnRefusal,
    /// No intent both refuses and plans.
    NoPlanAfterRefusal,
    /// No intent both refuses and actuates. The headline property.
    NoActuationAfterRefusal,
}

impl Invariant {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TraceId => "trace_id present on every event",
            Self::TerminalReconstructable => "every intent reaches a terminal event",
            Self::PolicyIdOnRefusal => "every refusal names its policy",
            Self::NoPlanAfterRefusal => "no planning after a refusal",
            Self::NoActuationAfterRefusal => "no actuation after a refusal",
        }
    }
}

pub const ALL: [Invariant; 5] = [
    Invariant::TraceId,
    Invariant::TerminalReconstructable,
    Invariant::PolicyIdOnRefusal,
    Invariant::NoPlanAfterRefusal,
    Invariant::NoActuationAfterRefusal,
];

/// `true` means the invariant holds over this log.
///
/// `in_flight` is the intent currently being processed, if any. It is excluded
/// from the terminal-event check for the obvious reason: a log read while the
/// system is still running always has exactly one intent that has not finished
/// yet, and calling that a violation would leave the panel permanently red —
/// which teaches a reader to ignore the one signal that should never be
/// ignored. Offline analysis of a completed run passes `None`.
pub fn check(log: &EventLog, inv: Invariant, in_flight: Option<IntentId>) -> bool {
    match inv {
        Invariant::TraceId => log.events.iter().all(|e| !e.trace.is_empty()),

        Invariant::TerminalReconstructable => log
            .intent_ids()
            .into_iter()
            .filter(|id| Some(*id) != in_flight)
            .all(|id| log.for_intent(id).any(|e| e.kind.is_terminal())),

        Invariant::PolicyIdOnRefusal => log
            .events
            .iter()
            .filter(|e| e.kind == EventKind::PolicyRefuse)
            .all(|e| e.policy.is_some()),

        Invariant::NoPlanAfterRefusal => log.intent_ids().into_iter().all(|id| {
            let refused = log
                .for_intent(id)
                .any(|e| e.kind == EventKind::PolicyRefuse);
            let planned = log
                .for_intent(id)
                .any(|e| matches!(e.kind, EventKind::StatePlanning | EventKind::PlanComputed));
            !(refused && planned)
        }),

        Invariant::NoActuationAfterRefusal => log.intent_ids().into_iter().all(|id| {
            let refused = log
                .for_intent(id)
                .any(|e| e.kind == EventKind::PolicyRefuse);
            let actuated = log.for_intent(id).any(|e| e.kind.is_actuation());
            !(refused && actuated)
        }),
    }
}

/// Every invariant, in declaration order, paired with whether it holds.
pub fn check_all(log: &EventLog, in_flight: Option<IntentId>) -> Vec<(Invariant, bool)> {
    ALL.iter().map(|&i| (i, check(log, i, in_flight))).collect()
}
