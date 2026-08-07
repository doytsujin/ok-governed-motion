//! The refusal guarantee, asserted rather than assumed.
//!
//! The headline claim is that a refused intent cannot reach the manipulator.
//! The type system already makes the direct route impossible — `plan` and
//! `begin` need an `Approved`, and only `policy::evaluate` mints one — so what
//! is left to test is that the *lifecycle* never routes a refusal past the
//! gate either.

use gov::{
    fsm::LastDecision,
    invariant::{self, Invariant},
    Agent, Approved, EventLog, Intent, Manipulator, State, WorldFacts,
};

/// A manipulator that records being touched. It cannot distinguish an
/// authorised call from an unauthorised one — that is the point: it does not
/// have to, because it cannot be called without an `Approved`.
#[derive(Default)]
struct Spy {
    planned: u32,
    begun: u32,
    ticks: u32,
    busy_for: u32,
}

impl Manipulator for Spy {
    fn plan(&mut self, _a: &Approved, _i: &Intent) -> u32 {
        self.planned += 1;
        2
    }
    fn begin(&mut self, _a: &Approved) {
        self.begun += 1;
        self.busy_for = 6;
    }
    fn busy(&self) -> bool {
        self.busy_for > 0
    }
    fn tick(&mut self, _dt: f32) {
        self.ticks += 1;
        self.busy_for = self.busy_for.saturating_sub(1);
    }
}

fn run(intent: Intent, facts: WorldFacts, steps: u32) -> (Agent, Spy, EventLog) {
    let mut agent = Agent::new("arm");
    let mut spy = Spy::default();
    let mut log = EventLog::default();
    agent.submit(intent);
    let dt = 0.1;
    for i in 0..steps {
        agent.tick(dt, i as f32 * dt, &facts, &mut spy, &mut log);
    }
    (agent, spy, log)
}

#[test]
fn a_refused_intent_never_reaches_the_manipulator() {
    let facts = WorldFacts {
        human_in_cell: true,
        ..Default::default()
    };
    let (agent, spy, log) = run(Intent::new(1, 0, 1, 7), facts, 60);

    assert_eq!(spy.planned, 0, "planner was invoked for a refused intent");
    assert_eq!(spy.begun, 0, "driver was commanded for a refused intent");
    assert_eq!(agent.state, State::Idle);
    assert_eq!(agent.refused, 1);
    assert_eq!(agent.completed, 0);
    assert!(matches!(agent.last, LastDecision::Refused { .. }));
    assert!(invariant::check(
        &log,
        Invariant::NoActuationAfterRefusal,
        None
    ));
    assert!(invariant::check(&log, Invariant::NoPlanAfterRefusal, None));
}

#[test]
fn the_lifecycle_never_enters_planning_or_executing_on_a_refusal() {
    let facts = WorldFacts {
        human_in_cell: true,
        ..Default::default()
    };
    let mut agent = Agent::new("arm");
    let mut spy = Spy::default();
    let mut log = EventLog::default();
    agent.submit(Intent::new(1, 0, 1, 7));

    let mut seen = Vec::new();
    for i in 0..60 {
        agent.tick(0.1, i as f32 * 0.1, &facts, &mut spy, &mut log);
        seen.push(agent.state);
    }
    assert!(
        !seen.contains(&State::Planning),
        "entered Planning on a refusal"
    );
    assert!(
        !seen.contains(&State::Executing),
        "entered Executing on a refusal"
    );
    assert!(seen.contains(&State::Reasoning), "never reasoned at all");
}

#[test]
fn an_approved_intent_does_reach_the_manipulator_and_completes() {
    let (agent, spy, log) = run(Intent::new(1, 0, 1, 7), WorldFacts::default(), 90);

    assert_eq!(spy.planned, 1);
    assert_eq!(spy.begun, 1);
    assert_eq!(agent.completed, 1, "approved intent never completed");
    assert_eq!(agent.refused, 0);
    assert_eq!(agent.state, State::Idle);
    for (inv, ok) in invariant::check_all(&log, None) {
        assert!(ok, "clean run violated: {}", inv.as_str());
    }
}

#[test]
fn every_event_carries_the_intents_trace_id() {
    let (_, _, log) = run(Intent::new(42, 0, 1, 7), WorldFacts::default(), 90);
    assert!(!log.events.is_empty());
    for e in &log.events {
        assert_eq!(e.trace, "tr-002a", "{} lost its trace", e.kind.as_str());
    }
}

#[test]
fn a_refusal_record_names_the_policy_that_refused_it() {
    let facts = WorldFacts {
        sterile_zone_locked: true,
        ..Default::default()
    };
    // Station 2 is the sterile bench.
    let (_, _, log) = run(Intent::new(1, 0, 2, 7), facts, 60);
    let refusal = log
        .events
        .iter()
        .find(|e| e.kind == gov::EventKind::PolicyRefuse)
        .expect("no refusal recorded");
    assert_eq!(
        refusal.policy.map(|p| p.as_str()),
        Some("STERILE_ZONE_LOCKED")
    );
}
