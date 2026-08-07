//! A two-link planar manipulator moving plates between four stations.
//!
//! Deliberately small: analytic inverse kinematics, a joint-space rate limit,
//! and nothing else. This exists so there is something real to govern and to
//! draw — the interesting claim is in [`gov`], not here.
//!
//! Note what implementing [`gov::Manipulator`] costs: both methods that move
//! the arm demand an `Approved`, and there is no other way in. The arm cannot
//! be commanded by code that did not go through the policy gate.

use gov::{Approved, Intent, Manipulator, N_STATIONS};

/// Link lengths, in the arbitrary units the web layer scales to pixels.
const L1: f32 = 0.45;
const L2: f32 = 0.40;
/// Joint rate limit. The arm being visibly slow is the point: a refusal has to
/// be observable as motion that never starts.
const RATE: f32 = 1.35;
/// Close enough to a joint target to call it reached.
const EPS: f32 = 0.012;

#[derive(Debug, Clone, Copy)]
pub struct Station {
    pub x: f32,
    pub y: f32,
    pub name: &'static str,
    pub sterile: bool,
}

/// Spread across the arm's reachable arc, all at positive `y`: the workspace
/// is a half-disc above the base, so a station below it would be both
/// unreachable and undrawable.
pub const STATIONS: [Station; N_STATIONS] = [
    Station {
        x: -0.66,
        y: 0.18,
        name: "incubator",
        sterile: false,
    },
    Station {
        x: 0.66,
        y: 0.18,
        name: "reader",
        sterile: false,
    },
    Station {
        x: -0.30,
        y: 0.68,
        name: "sterile bench",
        sterile: true,
    },
    Station {
        x: 0.40,
        y: 0.62,
        name: "waste",
        sterile: false,
    },
];

/// Which stations sit inside the sterile zone, read straight off [`STATIONS`].
///
/// `gov` cannot know this — it holds no geometry — so the fact has to be handed
/// to it. Deriving it here rather than writing the array twice is what stops
/// the policy and the picture disagreeing after someone moves a station.
pub fn sterile_flags() -> [bool; N_STATIONS] {
    let mut out = [false; N_STATIONS];
    for (i, s) in STATIONS.iter().enumerate() {
        out[i] = s.sterile;
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Action {
    Grasp(usize),
    Release(usize),
}

#[derive(Debug, Clone, Copy)]
struct Step {
    t1: f32,
    t2: f32,
    action: Action,
}

pub struct Workcell {
    pub t1: f32,
    pub t2: f32,
    /// Which plate is sitting at each station, if any.
    pub plate_at: [Option<u32>; N_STATIONS],
    pub held: Option<u32>,
    plan: Vec<Step>,
    step: usize,
    running: bool,
}

impl Default for Workcell {
    fn default() -> Self {
        Self::new()
    }
}

impl Workcell {
    pub fn new() -> Self {
        let (t1, t2) = ik(0.0, 0.55);
        Self {
            t1,
            t2,
            plate_at: [Some(1), None, None, None],
            held: None,
            plan: Vec::new(),
            step: 0,
            running: false,
        }
    }

    /// Elbow joint position, for drawing.
    pub fn elbow(&self) -> (f32, f32) {
        (L1 * self.t1.cos(), L1 * self.t1.sin())
    }

    /// End-effector position, for drawing.
    pub fn tip(&self) -> (f32, f32) {
        let (ex, ey) = self.elbow();
        let a = self.t1 + self.t2;
        (ex + L2 * a.cos(), ey + L2 * a.sin())
    }

    pub fn occupied(&self) -> [bool; N_STATIONS] {
        let mut out = [false; N_STATIONS];
        for (i, p) in self.plate_at.iter().enumerate() {
            out[i] = p.is_some();
        }
        out
    }

    fn advance_to(&mut self, t1: f32, t2: f32, dt: f32) -> bool {
        let max = RATE * dt;
        let (d1, d2) = (t1 - self.t1, t2 - self.t2);
        let dist = (d1 * d1 + d2 * d2).sqrt();
        if dist <= max.max(EPS) {
            self.t1 = t1;
            self.t2 = t2;
            return true;
        }
        let k = max / dist;
        self.t1 += d1 * k;
        self.t2 += d2 * k;
        false
    }
}

impl Manipulator for Workcell {
    fn plan(&mut self, _approved: &Approved, intent: &Intent) -> u32 {
        let src = STATIONS[intent.source];
        let dst = STATIONS[intent.dest];
        let (s1, s2) = ik(src.x, src.y);
        let (d1, d2) = ik(dst.x, dst.y);
        self.plan = vec![
            Step {
                t1: s1,
                t2: s2,
                action: Action::Grasp(intent.source),
            },
            Step {
                t1: d1,
                t2: d2,
                action: Action::Release(intent.dest),
            },
        ];
        self.step = 0;
        self.plan.len() as u32
    }

    fn begin(&mut self, _approved: &Approved) {
        self.running = !self.plan.is_empty();
    }

    fn busy(&self) -> bool {
        self.running
    }

    fn tick(&mut self, dt: f32) {
        if !self.running {
            return;
        }
        let Some(&step) = self.plan.get(self.step) else {
            self.running = false;
            return;
        };
        if !self.advance_to(step.t1, step.t2, dt) {
            return;
        }
        match step.action {
            Action::Grasp(i) => self.held = self.plate_at[i].take(),
            Action::Release(i) => self.plate_at[i] = self.held.take(),
        }
        self.step += 1;
        if self.step >= self.plan.len() {
            self.running = false;
        }
    }
}

/// Analytic IK for a two-link planar arm, elbow-up.
///
/// Targets beyond reach are clamped onto the workspace boundary rather than
/// producing a NaN — a manipulator that silently emits NaN joint angles is a
/// worse failure than one that stops short.
pub fn ik(x: f32, y: f32) -> (f32, f32) {
    let r2 = x * x + y * y;
    let r = r2.sqrt();
    let reach = L1 + L2;
    let (x, y, r, r2) = if r > reach * 0.999 {
        let k = reach * 0.999 / r;
        (
            x * k,
            y * k,
            reach * 0.999,
            (reach * 0.999) * (reach * 0.999),
        )
    } else {
        (x, y, r, r2)
    };
    let _ = r;
    let c2 = ((r2 - L1 * L1 - L2 * L2) / (2.0 * L1 * L2)).clamp(-1.0, 1.0);
    let t2 = c2.acos();
    let t1 = y.atan2(x) - (L2 * t2.sin()).atan2(L1 + L2 * t2.cos());
    (t1, t2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fk(t1: f32, t2: f32) -> (f32, f32) {
        let a = t1 + t2;
        (L1 * t1.cos() + L2 * a.cos(), L1 * t1.sin() + L2 * a.sin())
    }

    #[test]
    fn ik_round_trips_through_forward_kinematics() {
        for s in STATIONS {
            let (t1, t2) = ik(s.x, s.y);
            let (x, y) = fk(t1, t2);
            assert!(
                (x - s.x).abs() < 1e-3 && (y - s.y).abs() < 1e-3,
                "{}: wanted ({:.3},{:.3}) got ({x:.3},{y:.3})",
                s.name,
                s.x,
                s.y
            );
        }
    }

    #[test]
    fn out_of_reach_targets_clamp_instead_of_producing_nan() {
        let (t1, t2) = ik(9.0, 9.0);
        assert!(t1.is_finite() && t2.is_finite());
    }

    #[test]
    fn every_station_is_reachable() {
        for s in STATIONS {
            assert!(
                (s.x * s.x + s.y * s.y).sqrt() < L1 + L2,
                "{} is outside the workspace",
                s.name
            );
        }
    }
}
