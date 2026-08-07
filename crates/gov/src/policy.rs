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
