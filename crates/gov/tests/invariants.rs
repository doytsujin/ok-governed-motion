//! Fault discrimination.
//!
//! A checker that only ever sees compliant logs proves nothing about itself.
//! Each fault must drop **exactly** its own invariant: if injecting
//! `drop-trace` also broke `no-actuation-after-refusal`, the checker would be
//! reporting correlation rather than the property it claims to check.

use gov::{
    fault::{self, Fault},
    invariant::{self, Invariant},
    Agent, Approved, EventLog, Intent, Manipulator, WorldFacts,
};

#[derive(Default)]
struct Stub {
    busy_for: u32,
}

impl Manipulator for Stub {
    fn plan(&mut self, _a: &Approved, _i: &Intent) -> u32 {
        2
    }
    fn begin(&mut self, _a: &Approved) {
        self.busy_for = 5;
    }
    fn busy(&self) -> bool {
        self.busy_for > 0
    }
    fn tick(&mut self, _dt: f32) {
        self.busy_for = self.busy_for.saturating_sub(1);
    }
}

/// A run with both an approved and a refused intent, so every invariant has
/// something to bite on.
fn mixed_log() -> EventLog {
    let mut agent = Agent::new("arm");
    let mut robot = Stub::default();
    let mut log = EventLog::default();
    let clean = WorldFacts::default();
    let blocked = WorldFacts {
        human_in_cell: true,
        ..Default::default()
    };

    agent.submit(Intent::new(1, 0, 1, 7));
    for i in 0..90 {
        agent.tick(0.1, i as f32 * 0.1, &clean, &mut robot, &mut log);
    }
    agent.submit(Intent::new(2, 1, 3, 7));
    for i in 90..150 {
        agent.tick(0.1, i as f32 * 0.1, &blocked, &mut robot, &mut log);
    }

    // A third intent, submitted with the authority unreachable. Without it the
    // two indeterminate invariants would pass vacuously, which is the failure
    // mode this whole test file exists to avoid.
    agent.evaluator = gov::Evaluator {
        reachable: false,
        latency_s: 0.0,
    };
    agent.submit(Intent::new(3, 0, 1, 7));
    for i in 150..210 {
        agent.tick(0.1, i as f32 * 0.1, &clean, &mut robot, &mut log);
    }
    log
}

fn expected_victim(f: Fault) -> Option<Invariant> {
    match f {
        Fault::None => None,
        Fault::DropTrace => Some(Invariant::TraceId),
        Fault::NoTerminal => Some(Invariant::TerminalReconstructable),
        Fault::NoPolicyId => Some(Invariant::PolicyIdOnRefusal),
        Fault::LeakPlanner => Some(Invariant::NoPlanAfterRefusal),
        Fault::LeakDriver => Some(Invariant::NoActuationAfterRefusal),
        Fault::NoIndeterminateReason => Some(Invariant::ReasonOnIndeterminate),
        // Removing the terminal record of an unanswered intent leaves a gap,
        // and a gap is what `TerminalReconstructable` is for. This is the
        // fault that shows the invariant is satisfied by a record rather than
        // by the intent having quietly ended.
        Fault::NoIndeterminateTerminal => Some(Invariant::TerminalReconstructable),
    }
}

#[test]
fn the_unperturbed_log_satisfies_every_invariant() {
    let log = mixed_log();
    // Guard against a vacuous pass: the log must actually contain both a
    // completion and a refusal, or the invariants have nothing to check.
    assert!(log
        .events
        .iter()
        .any(|e| e.kind == gov::EventKind::IntentComplete));
    assert!(log
        .events
        .iter()
        .any(|e| e.kind == gov::EventKind::PolicyRefuse));
    assert!(log
        .events
        .iter()
        .any(|e| e.kind == gov::EventKind::PolicyIndeterminate));

    for (inv, ok) in invariant::check_all(&log, None) {
        assert!(ok, "clean log violated: {}", inv.as_str());
    }
}

#[test]
fn each_fault_drops_exactly_its_own_invariant() {
    let clean = mixed_log();
    for f in fault::ALL {
        let corrupted = fault::inject(&clean, f);
        let victim = expected_victim(f);
        for (inv, ok) in invariant::check_all(&corrupted, None) {
            let should_hold = Some(inv) != victim;
            assert_eq!(
                ok,
                should_hold,
                "fault {}: invariant '{}' was {}, expected {}",
                f.as_str(),
                inv.as_str(),
                if ok { "held" } else { "dropped" },
                if should_hold { "held" } else { "dropped" },
            );
        }
    }
}

#[test]
fn injecting_a_fault_does_not_disturb_the_original_log() {
    let clean = mixed_log();
    let before = clean.events.len();
    for f in fault::ALL {
        let _ = fault::inject(&clean, f);
    }
    assert_eq!(clean.events.len(), before, "inject mutated its input");
    for (inv, ok) in invariant::check_all(&clean, None) {
        assert!(ok, "original log damaged: {}", inv.as_str());
    }
}
