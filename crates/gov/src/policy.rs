//! Policy evaluation, and the token that proves it happened.
//!
//! The five policy classes are the ones exercised in the CBS 2026 evaluation.
//! Unlike that simulator, which marks an intent as violating up front, these
//! are evaluated against **live workcell state** — so locking the sterile zone
//! or stepping into the cell changes what the next intent is allowed to do.

use crate::{Intent, N_STATIONS};

/// Evaluated in this order; the first match refuses. Order is part of the
/// contract: a refusal record names exactly one policy, and which one it names
/// must not depend on iteration order of a map.
pub const POLICY_IDS: [PolicyId; 5] = [
    PolicyId::SterileZoneLocked,
    PolicyId::FixtureOccupied,
    PolicyId::DwellTimeExceeded,
    PolicyId::HumanLockout,
    PolicyId::MissingDescriptor,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyId {
    SterileZoneLocked,
    FixtureOccupied,
    DwellTimeExceeded,
    HumanLockout,
    MissingDescriptor,
}

impl PolicyId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SterileZoneLocked => "STERILE_ZONE_LOCKED",
            Self::FixtureOccupied => "FIXTURE_OCCUPIED",
            Self::DwellTimeExceeded => "DWELL_TIME_EXCEEDED",
            Self::HumanLockout => "HUMAN_LOCKOUT",
            Self::MissingDescriptor => "MISSING_DESCRIPTOR",
        }
    }

    /// Why this intent was refused, in the words an operator needs. A refusal
    /// that does not say which rule and why is an outage, not a decision.
    pub fn rationale(self) -> &'static str {
        match self {
            Self::SterileZoneLocked => "destination is inside a locked sterile zone",
            Self::FixtureOccupied => "destination fixture already holds a plate",
            Self::DwellTimeExceeded => "dwell time exceeds the limit for this sample",
            Self::HumanLockout => "an operator is present in the workspace",
            Self::MissingDescriptor => "intent carries no descriptor to govern it by",
        }
    }
}

/// What the environment says at the moment of evaluation.
#[derive(Debug, Clone)]
pub struct WorldFacts {
    pub sterile_zone_locked: bool,
    pub in_sterile_zone: [bool; N_STATIONS],
    pub occupied: [bool; N_STATIONS],
    pub human_in_cell: bool,
    pub max_dwell_s: f32,
}

impl Default for WorldFacts {
    fn default() -> Self {
        Self {
            sterile_zone_locked: false,
            in_sterile_zone: [false, false, true, false],
            occupied: [false; N_STATIONS],
            human_in_cell: false,
            max_dwell_s: 8.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub policy: PolicyId,
}

/// Prevents [`Approved`] being constructed anywhere but this module — a
/// private type in a public struct is unnameable from outside, so no other
/// code can write the literal, not even elsewhere in this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seal;

/// Proof that a policy authority approved a specific intent.
///
/// [`crate::Manipulator::begin`] takes one by reference, and nothing else can
/// start motion. Since only [`evaluate`] can mint one, the sentence "the
/// driver ran on a refused intent" describes a program that does not compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approved {
    intent: crate::IntentId,
    trace: String,
    _seal: Seal,
}

impl Approved {
    pub fn intent(&self) -> crate::IntentId {
        self.intent
    }

    pub fn trace(&self) -> &str {
        &self.trace
    }
}

/// Why the authority could not answer.
///
/// Deliberately not a [`PolicyId`]. No policy refused this intent; the thing
/// that could have refused it did not answer, and collapsing the two would put
/// a rule's name on an outcome no rule produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndeterminateReason {
    /// The authority was not reachable at all.
    EvaluatorUnavailable,
    /// The authority was reachable and did not answer inside the budget.
    EvaluatorTimeout,
}

impl IndeterminateReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EvaluatorUnavailable => "EVALUATOR_UNAVAILABLE",
            Self::EvaluatorTimeout => "EVALUATOR_TIMEOUT",
        }
    }

    pub fn rationale(self) -> &'static str {
        match self {
            Self::EvaluatorUnavailable => "the policy authority could not be reached",
            Self::EvaluatorTimeout => "the policy authority did not answer within the budget",
        }
    }
}

/// An outcome that is neither permission nor refusal.
///
/// It exists so that "nobody decided" is a row rather than a hole. A fail-open
/// and an approval leave the same trace three months later if the only evidence
/// of non-evaluation is the absence of evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Indeterminate {
    pub reason: IndeterminateReason,
}

/// The three things adjudication can conclude.
///
/// `Indeterminate` is terminal in exactly the way `Refused` is: no plan, no
/// driver command, no [`Approved`]. What separates them is the record, and an
/// operator reading it needs to tell "a rule said no" from "no rule answered".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Approved(Approved),
    Refused(Refusal),
    Indeterminate(Indeterminate),
}

impl Verdict {
    /// Only an approval yields the token that starts motion. Both other arms
    /// return `None`, which is the whole point: indeterminacy is not a
    /// weaker approval.
    pub fn approved(self) -> Option<Approved> {
        match self {
            Self::Approved(a) => Some(a),
            _ => None,
        }
    }
}

/// Whether the policy authority can answer, and how long it takes.
///
/// Separate from [`WorldFacts`] on purpose. The facts describe the cell; this
/// describes the governor. A system that cannot distinguish "the workspace is
/// occupied" from "I could not find out whether the workspace is occupied" is
/// the failure this type exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Evaluator {
    pub reachable: bool,
    pub latency_s: f32,
}

impl Default for Evaluator {
    fn default() -> Self {
        Self { reachable: true, latency_s: 0.0 }
    }
}

/// Adjudication: consult the authority, and if it cannot answer, say so.
///
/// [`evaluate`] is the policy decision and is a pure function of the intent and
/// the facts. This wraps it with the one thing a pure function cannot express —
/// that the authority may not be there. When it is not, the result is
/// [`Verdict::Indeterminate`] and no [`Approved`] is minted, so execution
/// remains unreachable by construction rather than by convention.
pub fn adjudicate(
    intent: &Intent,
    facts: &WorldFacts,
    evaluator: &Evaluator,
    budget_s: f32,
) -> Verdict {
    if !evaluator.reachable {
        return Verdict::Indeterminate(Indeterminate {
            reason: IndeterminateReason::EvaluatorUnavailable,
        });
    }
    if evaluator.latency_s > budget_s {
        return Verdict::Indeterminate(Indeterminate {
            reason: IndeterminateReason::EvaluatorTimeout,
        });
    }
    match evaluate(intent, facts) {
        Ok(a) => Verdict::Approved(a),
        Err(r) => Verdict::Refused(r),
    }
}

/// The reasoning phase. Runs before the planner exists, and is the only door
/// to [`Approved`].
pub fn evaluate(intent: &Intent, facts: &WorldFacts) -> Result<Approved, Refusal> {
    for policy in POLICY_IDS {
        let violated = match policy {
            PolicyId::SterileZoneLocked => {
                facts.sterile_zone_locked && facts.in_sterile_zone[intent.dest]
            }
            PolicyId::FixtureOccupied => facts.occupied[intent.dest],
            PolicyId::DwellTimeExceeded => intent.dwell_s > facts.max_dwell_s,
            PolicyId::HumanLockout => facts.human_in_cell,
            PolicyId::MissingDescriptor => !intent.descriptor,
        };
        if violated {
            return Err(Refusal { policy });
        }
    }
    Ok(Approved {
        intent: intent.id,
        trace: intent.trace.clone(),
        _seal: Seal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_intent_is_approved() {
        let i = Intent::new(1, 0, 1, 7);
        assert!(evaluate(&i, &WorldFacts::default()).is_ok());
    }

    #[test]
    fn each_policy_refuses_on_its_own_condition() {
        let base = Intent::new(1, 0, 2, 7);
        let cases: [(PolicyId, WorldFacts, Intent); 5] = [
            (
                PolicyId::SterileZoneLocked,
                WorldFacts {
                    sterile_zone_locked: true,
                    ..Default::default()
                },
                base.clone(),
            ),
            (
                PolicyId::FixtureOccupied,
                WorldFacts {
                    occupied: [false, false, true, false],
                    ..Default::default()
                },
                base.clone(),
            ),
            (
                PolicyId::DwellTimeExceeded,
                WorldFacts::default(),
                base.clone().with_dwell(99.0),
            ),
            (
                PolicyId::HumanLockout,
                WorldFacts {
                    human_in_cell: true,
                    ..Default::default()
                },
                base.clone(),
            ),
            (
                PolicyId::MissingDescriptor,
                WorldFacts::default(),
                base.clone().without_descriptor(),
            ),
        ];
        for (want, facts, intent) in cases {
            let got = evaluate(&intent, &facts).expect_err("should refuse");
            assert_eq!(got.policy, want, "wrong policy named for {want:?}");
        }
    }

    #[test]
    fn refusal_names_the_first_policy_in_order_not_an_arbitrary_one() {
        // Two conditions hold at once. The record must be deterministic, or a
        // refusal rationale becomes unreproducible across runs.
        let facts = WorldFacts {
            sterile_zone_locked: true,
            human_in_cell: true,
            ..Default::default()
        };
        let i = Intent::new(1, 0, 2, 7).without_descriptor();
        assert_eq!(
            evaluate(&i, &facts).unwrap_err().policy,
            PolicyId::SterileZoneLocked
        );
    }
}
