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

// ── An outcome nobody produced ───────────────────────────────────────────
//
// Added 2026-08-27 after a reader asked whether the control plane writes a
// positive record of non-evaluation, or infers an unavailable authority from a
// gap in the trace. It inferred. These tests are the answer.

/// As `run`, with the policy authority taken away.
fn run_unreachable(intent: Intent, facts: WorldFacts, steps: u32) -> (Agent, Spy, EventLog) {
    let mut agent = Agent::new("arm");
    agent.evaluator = gov::Evaluator {
        reachable: false,
        latency_s: 0.0,
    };
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
fn an_unanswered_intent_never_reaches_the_manipulator() {
    // The property that makes fail-open impossible rather than unlikely: no
    // `Approved` is minted, and `begin` cannot be called without one.
    let (agent, spy, _log) = run_unreachable(Intent::new(1, 0, 1, 7), WorldFacts::default(), 60);
    assert_eq!(spy.planned, 0, "planner was invoked for an unanswered intent");
    assert_eq!(spy.begun, 0, "driver was commanded for an unanswered intent");
    assert_eq!(agent.indeterminate, 1);
    assert_eq!(agent.completed, 0);
}

#[test]
fn an_unanswered_intent_writes_a_record_rather_than_a_gap() {
    // The question, precisely. An intent the authority could not answer must
    // leave a terminal row saying so -- not silence that a reader three months
    // later cannot distinguish from an approval, or from never having been
    // submitted at all.
    let (_, _, log) = run_unreachable(Intent::new(1, 0, 1, 7), WorldFacts::default(), 60);

    let decision = log
        .events
        .iter()
        .find(|e| e.kind == gov::EventKind::PolicyIndeterminate)
        .expect("no indeterminate decision recorded");
    assert_eq!(
        decision.reason.map(|r| r.as_str()),
        Some("EVALUATOR_UNAVAILABLE")
    );
    assert!(decision.policy.is_none(), "no policy refused this intent");

    assert!(
        log.events
            .iter()
            .any(|e| e.kind == gov::EventKind::IntentIndeterminate),
        "the intent has no terminal record"
    );
    assert!(
        log.for_intent(1).any(|e| e.kind.is_terminal()),
        "an unanswered intent must still reach a terminal event"
    );
}

#[test]
fn an_unanswered_intent_is_not_recorded_as_a_refusal() {
    // The distinction the whole change exists for. "A rule said no" and "no
    // rule answered" are different facts, and an audit that conflates them
    // cannot tell a governed system from an unavailable one.
    let (agent, _, log) = run_unreachable(Intent::new(1, 0, 1, 7), WorldFacts::default(), 60);
    assert!(
        !log.events
            .iter()
            .any(|e| e.kind == gov::EventKind::PolicyRefuse),
        "an unanswered intent was recorded as a refusal"
    );
    assert_eq!(agent.refused, 0);
    assert!(matches!(
        agent.last,
        LastDecision::Indeterminate { .. }
    ));
}

#[test]
fn a_slow_authority_times_out_rather_than_being_waited_on() {
    let mut agent = Agent::new("arm");
    agent.evaluator = gov::Evaluator {
        reachable: true,
        latency_s: 1_000.0,
    };
    let mut spy = Spy::default();
    let mut log = EventLog::default();
    agent.submit(Intent::new(1, 0, 1, 7));
    for i in 0..60 {
        agent.tick(0.1, i as f32 * 0.1, &WorldFacts::default(), &mut spy, &mut log);
    }
    let decision = log
        .events
        .iter()
        .find(|e| e.kind == gov::EventKind::PolicyIndeterminate)
        .expect("a slow authority produced no record");
    assert_eq!(decision.reason.map(|r| r.as_str()), Some("EVALUATOR_TIMEOUT"));
    assert_eq!(spy.begun, 0, "driver ran while the authority was still thinking");
}
