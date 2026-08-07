//! The event log: what happened, to which intent, under which trace.
//!
//! Every event carries a trace id. That is what makes the causal chain
//! reconstructable after the fact, and it is the first thing the invariant
//! checker looks for — see [`crate::invariant`].

use crate::{policy::PolicyId, IntentId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    MeshRoute,
    EnvResolve,
    StateIdle,
    StateReasoning,
    StatePlanning,
    StateExecuting,
    StatePublishing,
    PolicyOk,
    PolicyRefuse,
    IntentRefused,
    PlanComputed,
    DriverCommand,
    Telemetry,
    IntentComplete,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MeshRoute => "MESH_ROUTE",
            Self::EnvResolve => "ENV_RESOLVE",
            Self::StateIdle => "STATE_IDLE",
            Self::StateReasoning => "STATE_REASONING",
            Self::StatePlanning => "STATE_PLANNING",
            Self::StateExecuting => "STATE_EXECUTING",
            Self::StatePublishing => "STATE_PUBLISHING",
            Self::PolicyOk => "POLICY_OK",
            Self::PolicyRefuse => "POLICY_REFUSE",
            Self::IntentRefused => "INTENT_REFUSED",
            Self::PlanComputed => "PLAN_COMPUTED",
            Self::DriverCommand => "DRIVER_COMMAND",
            Self::Telemetry => "TELEMETRY",
            Self::IntentComplete => "INTENT_COMPLETE",
        }
    }

    /// The two events that mean a manipulator was actually driven. The
    /// refusal invariant is stated in terms of these.
    pub fn is_actuation(self) -> bool {
        matches!(self, Self::StateExecuting | Self::DriverCommand)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::IntentRefused | Self::IntentComplete)
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub t: f32,
    pub kind: EventKind,
    pub source: String,
    pub intent: IntentId,
    /// Empty only when a fault has been injected to remove it.
    pub trace: String,
    pub policy: Option<PolicyId>,
}

#[derive(Debug, Default, Clone)]
pub struct EventLog {
    pub events: Vec<Event>,
}

impl EventLog {
    pub fn record(&mut self, t: f32, kind: EventKind, source: &str, intent: IntentId, trace: &str) {
        self.events.push(Event {
            t,
            kind,
            source: source.to_string(),
            intent,
            trace: trace.to_string(),
            policy: None,
        });
    }

    pub fn record_policy(
        &mut self,
        t: f32,
        kind: EventKind,
        intent: IntentId,
        trace: &str,
        policy: PolicyId,
    ) {
        self.events.push(Event {
            t,
            kind,
            source: "policy_authority".to_string(),
            intent,
            trace: trace.to_string(),
            policy: Some(policy),
        });
    }

    pub fn for_intent(&self, intent: IntentId) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |e| e.intent == intent)
    }

    pub fn intent_ids(&self) -> Vec<IntentId> {
        let mut ids: Vec<IntentId> = self.events.iter().map(|e| e.intent).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}
