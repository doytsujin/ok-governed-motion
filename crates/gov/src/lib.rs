//! A supervisory control plane for governed motion.
//!
//! An *intent* is a structured request to move something. It is evaluated
//! against policy during a dedicated reasoning phase, **before** any planner
//! or driver is touched. A refused intent returns to `Idle` without ever
//! entering `Planning` or `Executing`, so "no actuation on a refused intent"
//! is a property of the architecture rather than a promise made by careful
//! code.
//!
//! This crate takes that one step further than a runtime check can. The
//! planner and driver entry points require an [`Approved`] token, and the only
//! code that can mint one is the policy authority — see [`policy`]. Actuating
//! on a refused intent is therefore not a bug you could write and catch in
//! review; it is a program that does not compile.
//!
//! Deliberately dependency-free, including the RNG, so the guarantee can be
//! audited by reading roughly a thousand lines.

pub mod fault;
pub mod fsm;
pub mod invariant;
pub mod log;
pub mod policy;

pub use fault::Fault;
pub use fsm::{Agent, Manipulator, State};
pub use log::{Event, EventKind, EventLog};
pub use policy::{Approved, PolicyId, Refusal, WorldFacts, POLICY_IDS};

/// Stations in the workcell. Kept as a plain index so this crate stays free of
/// geometry — where station 2 actually *is* belongs to the robot, not to the
/// question of whether moving there is allowed.
pub const N_STATIONS: usize = 4;

pub type IntentId = u32;

/// A governed intent: not a bare motion request, but everything policy needs
/// in order to decide *before* anything moves.
#[derive(Debug, Clone, PartialEq)]
pub struct Intent {
    pub id: IntentId,
    /// Correlates every event this intent produces. The invariant checker
    /// requires it to be present on all of them.
    pub trace: String,
    pub source: usize,
    pub dest: usize,
    pub plate: u32,
    /// A governed intent carries its descriptor; one that does not is refused
    /// rather than executed on faith.
    pub descriptor: bool,
    pub operator_authorized: bool,
    /// How long the sample would sit outside its controlled environment.
    pub dwell_s: f32,
}

impl Intent {
    pub fn new(id: IntentId, source: usize, dest: usize, plate: u32) -> Self {
        Self {
            id,
            trace: format!("tr-{id:04x}"),
            source,
            dest,
            plate,
            descriptor: true,
            operator_authorized: true,
            dwell_s: 4.0,
        }
    }

    pub fn without_descriptor(mut self) -> Self {
        self.descriptor = false;
        self
    }

    pub fn with_dwell(mut self, s: f32) -> Self {
        self.dwell_s = s;
        self
    }
}

/// Deterministic PRNG. A visualisation people re-run has to produce the same
/// story twice, and a seeded xorshift is the whole of what is needed.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // A zero state is a fixed point of xorshift, so it can never be used.
        Self(if seed == 0 { 0x9E3779B97F4A7C15 } else { seed })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        let u = (self.next_u64() >> 11) as f32 / (1u64 << 53) as f32;
        lo + u * (hi - lo)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}
