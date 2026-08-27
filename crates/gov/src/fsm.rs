//! The lifecycle FSM.
//!
//! `Idle → Reasoning → Planning → Executing → Publishing → Idle`, with a
//! refusal in `Reasoning` returning straight to `Idle`. Planning and executing
//! are the only phases with side effects, and they sit past the gate, so the
//! refusal guarantee falls out of the shape rather than out of vigilance.

use crate::{
    log::{EventKind, EventLog},
    policy::{self, Approved, WorldFacts},
    Intent,
};

/// Phase durations in seconds. Wall-clock realism is not the point; being able
/// to *see* each phase is.
const REASON_S: f32 = 0.45;
const PLAN_S: f32 = 0.55;
const PUBLISH_S: f32 = 0.35;
const TELEMETRY_PERIOD_S: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Reasoning,
    Planning,
    Executing,
    Publishing,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Reasoning => "REASONING",
            Self::Planning => "PLANNING",
            Self::Executing => "EXECUTING",
            Self::Publishing => "PUBLISHING",
        }
    }

    fn event(self) -> EventKind {
        match self {
            Self::Idle => EventKind::StateIdle,
            Self::Reasoning => EventKind::StateReasoning,
            Self::Planning => EventKind::StatePlanning,
            Self::Executing => EventKind::StateExecuting,
            Self::Publishing => EventKind::StatePublishing,
        }
    }
}

/// The thing being governed.
///
/// Both side-effecting methods demand an [`Approved`]. That is the enforcement
/// point: an implementor cannot offer a way to move that skips the gate
/// without changing this trait, which is a visible act rather than an
/// oversight.
pub trait Manipulator {
    /// Compute a trajectory. Returns the number of waypoints, purely so the
    /// plan event has something to report.
    fn plan(&mut self, approved: &Approved, intent: &Intent) -> u32;
    /// Start moving along the planned trajectory.
    fn begin(&mut self, approved: &Approved);
    /// Still moving?
    fn busy(&self) -> bool;
    /// Advance physical state.
    fn tick(&mut self, dt: f32);
}

/// The outcome of the most recent decision, for display.
#[derive(Debug, Clone, PartialEq)]
pub enum LastDecision {
    None,
    Approved {
        intent: crate::IntentId,
    },
    Refused {
        intent: crate::IntentId,
        policy: policy::PolicyId,
    },
    /// Nobody decided. Kept apart from `Refused` so a caller cannot read one
    /// as the other, which is the same reason the log keeps them apart.
    Indeterminate {
        intent: crate::IntentId,
        reason: policy::IndeterminateReason,
    },
}

pub struct Agent {
    pub name: &'static str,
    pub state: State,
    pub queue: Vec<Intent>,
    pub last: LastDecision,
    pub completed: u32,
    pub refused: u32,
    pub indeterminate: u32,
    /// Whether the policy authority can answer. Settable so a scenario can take
    /// the governor away and show what the record does about it.
    pub evaluator: policy::Evaluator,
    timer: f32,
    telemetry_due: f32,
    current: Option<Intent>,
    /// Held only between approval and the end of execution. Its presence is
    /// the runtime shadow of the compile-time rule.
    approved: Option<Approved>,
}

impl Agent {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            state: State::Idle,
            queue: Vec::new(),
            last: LastDecision::None,
            completed: 0,
            refused: 0,
            indeterminate: 0,
            evaluator: policy::Evaluator::default(),
            timer: 0.0,
            telemetry_due: 0.0,
            current: None,
            approved: None,
        }
    }

    pub fn submit(&mut self, intent: Intent) {
        self.queue.push(intent);
    }

    pub fn current(&self) -> Option<&Intent> {
        self.current.as_ref()
    }

    fn transition(&mut self, to: State, t: f32, log: &mut EventLog) {
        self.state = to;
        let (id, trace) = match &self.current {
            Some(i) => (i.id, i.trace.clone()),
            None => (0, String::new()),
        };
        let source = format!("manipulator/{}", self.name);
        log.record(t, to.event(), &source, id, &trace);
    }

    /// One step of the lifecycle. `t` is the current simulation time.
    pub fn tick<M: Manipulator>(
        &mut self,
        dt: f32,
        t: f32,
        facts: &WorldFacts,
        robot: &mut M,
        log: &mut EventLog,
    ) {
        robot.tick(dt);
        self.timer -= dt;

        match self.state {
            State::Idle => {
                if self.queue.is_empty() {
                    return;
                }
                let intent = self.queue.remove(0);
                let (id, trace) = (intent.id, intent.trace.clone());
                self.current = Some(intent);
                log.record(t, EventKind::MeshRoute, "mesh", id, &trace);
                log.record(t, EventKind::EnvResolve, "environment", id, &trace);
                self.transition(State::Reasoning, t, log);
                self.timer = REASON_S;
            }

            State::Reasoning => {
                if self.timer > 0.0 {
                    return;
                }
                // The gate. Nothing below this point exists for a refusal.
                let intent = self.current.clone().expect("reasoning without an intent");
                match policy::adjudicate(&intent, facts, &self.evaluator, REASON_S) {
                    // Nobody decided. Terminal, recorded, and no `Approved`
                    // exists — so the driver is unreachable for exactly the
                    // reason it is unreachable after a refusal, and the log
                    // says which of the two happened.
                    policy::Verdict::Indeterminate(ind) => {
                        log.record_indeterminate(
                            t,
                            EventKind::PolicyIndeterminate,
                            intent.id,
                            &intent.trace,
                            ind.reason,
                        );
                        self.transition(State::Idle, t, log);
                        log.record_indeterminate(
                            t,
                            EventKind::IntentIndeterminate,
                            intent.id,
                            &intent.trace,
                            ind.reason,
                        );
                        self.indeterminate += 1;
                        self.last = LastDecision::Indeterminate {
                            intent: intent.id,
                            reason: ind.reason,
                        };
                        self.current = None;
                        self.approved = None;
                    }
                    policy::Verdict::Refused(refusal) => {
                        log.record_policy(
                            t,
                            EventKind::PolicyRefuse,
                            intent.id,
                            &intent.trace,
                            refusal.policy,
                        );
                        self.transition(State::Idle, t, log);
                        log.record(
                            t,
                            EventKind::IntentRefused,
                            "telemetry",
                            intent.id,
                            &intent.trace,
                        );
                        self.refused += 1;
                        self.last = LastDecision::Refused {
                            intent: intent.id,
                            policy: refusal.policy,
                        };
                        self.current = None;
                        self.approved = None;
                    }
                    policy::Verdict::Approved(approved) => {
                        log.record(
                            t,
                            EventKind::PolicyOk,
                            "policy_authority",
                            intent.id,
                            &intent.trace,
                        );
                        self.last = LastDecision::Approved { intent: intent.id };
                        self.approved = Some(approved);
                        self.transition(State::Planning, t, log);
                        self.timer = PLAN_S;
                    }
                }
            }

            State::Planning => {
                if self.timer > 0.0 {
                    return;
                }
                let intent = self.current.clone().expect("planning without an intent");
                let approved = self.approved.as_ref().expect("planning without approval");
                let waypoints = robot.plan(approved, &intent);
                log.record(
                    t,
                    EventKind::PlanComputed,
                    "planner",
                    intent.id,
                    &intent.trace,
                );
                let _ = waypoints;
                robot.begin(approved);
                log.record(
                    t,
                    EventKind::DriverCommand,
                    "driver",
                    intent.id,
                    &intent.trace,
                );
                self.transition(State::Executing, t, log);
                self.telemetry_due = TELEMETRY_PERIOD_S;
            }

            State::Executing => {
                let intent = self.current.clone().expect("executing without an intent");
                self.telemetry_due -= dt;
                if self.telemetry_due <= 0.0 {
                    self.telemetry_due = TELEMETRY_PERIOD_S;
                    log.record(t, EventKind::Telemetry, "driver", intent.id, &intent.trace);
                }
                if !robot.busy() {
                    self.transition(State::Publishing, t, log);
                    self.timer = PUBLISH_S;
                }
            }

            State::Publishing => {
                if self.timer > 0.0 {
                    return;
                }
                let intent = self.current.clone().expect("publishing without an intent");
                log.record(
                    t,
                    EventKind::IntentComplete,
                    "telemetry",
                    intent.id,
                    &intent.trace,
                );
                self.completed += 1;
                self.transition(State::Idle, t, log);
                self.current = None;
                self.approved = None;
            }
        }
    }
}
