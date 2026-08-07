//! Synthetic faults, applied to a *copy* of a recorded log.
//!
//! These do not break the running system — they corrupt its record, which is
//! what the invariant checker actually reads. The point is to show the checker
//! discriminates: each fault must drop its own invariant and no other. A
//! checker that passes everything and a checker that is correct look identical
//! until something is deliberately broken.

use crate::log::{Event, EventKind, EventLog};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    None,
    DropTrace,
    NoTerminal,
    NoPolicyId,
    LeakPlanner,
    LeakDriver,
}

impl Fault {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DropTrace => "drop-trace",
            Self::NoTerminal => "no-terminal",
            Self::NoPolicyId => "no-policy-id",
            Self::LeakPlanner => "leak-planner",
            Self::LeakDriver => "leak-driver",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::None => "unperturbed log",
            Self::DropTrace => "an event loses its trace id",
            Self::NoTerminal => "an intent never reaches a terminal event",
            Self::NoPolicyId => "a refusal is recorded without naming its policy",
            Self::LeakPlanner => "a refused intent shows planning activity",
            Self::LeakDriver => "a refused intent shows the driver being commanded",
        }
    }
}

pub const ALL: [Fault; 6] = [
    Fault::None,
    Fault::DropTrace,
    Fault::NoTerminal,
    Fault::NoPolicyId,
    Fault::LeakPlanner,
    Fault::LeakDriver,
];

/// Returns a corrupted copy. The original is never touched — a fault must not
/// be able to change the system's actual behaviour, only its record.
pub fn inject(log: &EventLog, fault: Fault) -> EventLog {
    let mut out = log.clone();
    match fault {
        Fault::None => {}

        Fault::DropTrace => {
            if let Some(e) = out.events.first_mut() {
                e.trace.clear();
            }
        }

        Fault::NoTerminal => {
            // Remove the terminal event of the first intent that has one.
            if let Some(idx) = out.events.iter().position(|e| e.kind.is_terminal()) {
                let victim = out.events[idx].intent;
                out.events
                    .retain(|e| !(e.intent == victim && e.kind.is_terminal()));
            }
        }

        Fault::NoPolicyId => {
            if let Some(e) = out
                .events
                .iter_mut()
                .find(|e| e.kind == EventKind::PolicyRefuse)
            {
                e.policy = None;
            }
        }

        Fault::LeakPlanner => leak(&mut out, EventKind::StatePlanning, "planner"),
        Fault::LeakDriver => leak(&mut out, EventKind::DriverCommand, "driver"),
    }
    out
}

/// Splice an event of `kind` into a refused intent's history — the shape a
/// broken implementation would leave behind if the gate did not hold.
fn leak(log: &mut EventLog, kind: EventKind, source: &str) {
    let refused = log
        .events
        .iter()
        .find(|e| e.kind == EventKind::PolicyRefuse)
        .map(|e| (e.intent, e.trace.clone(), e.t));
    if let Some((intent, trace, t)) = refused {
        log.events.push(Event {
            t: t + 0.01,
            kind,
            source: source.to_string(),
            intent,
            trace,
            policy: None,
        });
        log.events.sort_by(|a, b| a.t.total_cmp(&b.t));
    }
}
