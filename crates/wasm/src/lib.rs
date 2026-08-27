//! The seam the browser calls.
//!
//! JSON is hand-written rather than pulled in through a serialisation crate:
//! the payload is a few dozen fields, and keeping the dependency list at
//! exactly one makes the claim "you can audit this" survive contact with
//! `cargo tree`.

use gov::{
    fault::{self, Fault},
    fsm::LastDecision,
    invariant, Agent, EventLog, Intent, Rng, WorldFacts, N_STATIONS,
};
use robot::{Workcell, STATIONS};
use wasm_bindgen::prelude::*;

/// How many events the page keeps. A trace panel nobody scrolls past 400
/// entries does not need unbounded memory.
const LOG_CAP: usize = 400;

#[wasm_bindgen]
pub struct Sim {
    agent: Agent,
    cell: Workcell,
    log: EventLog,
    facts: WorldFacts,
    rng: Rng,
    fault: Fault,
    t: f32,
    next_id: u32,
    auto: bool,
    auto_cooldown: f32,
}

#[wasm_bindgen]
impl Sim {
    #[wasm_bindgen(constructor)]
    pub fn new(seed: u32) -> Sim {
        // Geometry is the robot's to know; the supervisor is told which
        // stations are sterile rather than assuming a layout.
        let facts = WorldFacts {
            in_sterile_zone: robot::sterile_flags(),
            ..WorldFacts::default()
        };
        Sim {
            agent: Agent::new("arm"),
            cell: Workcell::new(),
            log: EventLog::default(),
            facts,
            rng: Rng::new(seed as u64),
            fault: Fault::None,
            t: 0.0,
            next_id: 1,
            auto: true,
            auto_cooldown: 0.0,
        }
    }

    pub fn tick(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        self.t += dt;

        // The environment is resolved fresh every tic: occupancy is a fact
        // about the cell, not a copy the supervisor keeps and lets go stale.
        self.facts.occupied = self.cell.occupied();

        if self.auto {
            self.auto_cooldown -= dt;
            if self.auto_cooldown <= 0.0 && self.agent.queue.is_empty() {
                self.auto_cooldown = self.rng.range(0.6, 1.6);
                self.enqueue_random();
            }
        }

        self.agent
            .tick(dt, self.t, &self.facts, &mut self.cell, &mut self.log);

        if self.log.events.len() > LOG_CAP {
            let drop = self.log.events.len() - LOG_CAP;
            self.log.events.drain(0..drop);
        }
    }

    /// Queue an intent moving whichever plate exists to a chosen station.
    fn enqueue_random(&mut self) {
        let from = (0..N_STATIONS).find(|&i| self.cell.plate_at[i].is_some());
        let Some(from) = from else { return };
        let mut to = self.rng.below(N_STATIONS);
        if to == from {
            to = (to + 1) % N_STATIONS;
        }
        let mut intent = Intent::new(self.next_id, from, to, 1);
        // Occasionally emit an ungoverned request, so MISSING_DESCRIPTOR is
        // reachable without the operator having to force it.
        if self.rng.range(0.0, 1.0) < 0.08 {
            intent = intent.without_descriptor();
        }
        intent = intent.with_dwell(self.rng.range(2.0, 10.0));
        self.next_id += 1;
        self.agent.submit(intent);
    }

    pub fn submit(&mut self, source: usize, dest: usize, descriptor: bool, dwell: f32) {
        if source >= N_STATIONS || dest >= N_STATIONS || source == dest {
            return;
        }
        let mut intent = Intent::new(self.next_id, source, dest, 1);
        intent.descriptor = descriptor;
        intent = intent.with_dwell(dwell);
        self.next_id += 1;
        self.agent.submit(intent);
    }

    pub fn set_auto(&mut self, on: bool) {
        self.auto = on;
    }

    pub fn set_human(&mut self, present: bool) {
        self.facts.human_in_cell = present;
    }

    pub fn set_sterile_locked(&mut self, locked: bool) {
        self.facts.sterile_zone_locked = locked;
    }

    pub fn set_max_dwell(&mut self, s: f32) {
        self.facts.max_dwell_s = s;
    }

    pub fn set_fault(&mut self, name: &str) {
        self.fault = fault::ALL
            .into_iter()
            .find(|f| f.as_str() == name)
            .unwrap_or(Fault::None);
    }

    /// Everything the renderer needs for one frame.
    pub fn snapshot(&self) -> String {
        let (ex, ey) = self.cell.elbow();
        let (tx, ty) = self.cell.tip();
        let last = match &self.agent.last {
            LastDecision::None => "\"none\":true".to_string(),
            LastDecision::Approved { intent } => {
                format!("\"approved\":true,\"intent\":{intent}")
            }
            LastDecision::Refused { intent, policy } => format!(
                "\"approved\":false,\"intent\":{intent},\"policy\":\"{}\",\"why\":\"{}\"",
                policy.as_str(),
                policy.rationale()
            ),
            // Rendered as its own case rather than folded into `approved:false`.
            // A viewer who cannot tell a refusal from an unanswered intent is
            // back to reading a fail-open as an approval.
            LastDecision::Indeterminate { intent, reason } => format!(
                "\"approved\":false,\"indeterminate\":true,\"intent\":{intent},\
                 \"policy\":\"{}\",\"why\":\"{}\"",
                reason.as_str(),
                reason.rationale()
            ),
        };
        let plates: Vec<String> = self
            .cell
            .plate_at
            .iter()
            .map(|p| match p {
                Some(_) => "true".to_string(),
                None => "false".to_string(),
            })
            .collect();
        let (cur_id, cur_src, cur_dst, cur_dwell) = match self.agent.current() {
            Some(i) => (i.id as i64, i.source as i64, i.dest as i64, i.dwell_s),
            None => (-1, -1, -1, 0.0),
        };
        format!(
            concat!(
                "{{\"t\":{:.2},\"state\":\"{}\",\"t1\":{:.4},\"t2\":{:.4},",
                "\"elbow\":[{:.4},{:.4}],\"tip\":[{:.4},{:.4}],\"held\":{},",
                "\"plates\":[{}],\"completed\":{},\"refused\":{},",
                "\"queued\":{},\"busy\":{},",
                "\"human\":{},\"sterileLocked\":{},\"maxDwell\":{:.1},",
                "\"current\":{{\"id\":{},\"source\":{},\"dest\":{},\"dwell\":{:.1}}},",
                "\"last\":{{{}}}}}"
            ),
            self.t,
            self.agent.state.as_str(),
            self.cell.t1,
            self.cell.t2,
            ex,
            ey,
            tx,
            ty,
            self.cell.held.is_some(),
            plates.join(","),
            self.agent.completed,
            self.agent.refused,
            self.agent.queue.len(),
            self.cell.held.is_some(),
            self.facts.human_in_cell,
            self.facts.sterile_zone_locked,
            self.facts.max_dwell_s,
            cur_id,
            cur_src,
            cur_dst,
            cur_dwell,
            last,
        )
    }

    /// The most recent `n` events, newest last.
    pub fn events(&self, n: usize) -> String {
        let start = self.log.events.len().saturating_sub(n);
        let rows: Vec<String> = self.log.events[start..]
            .iter()
            .map(|e| {
                format!(
                    "{{\"t\":{:.2},\"kind\":\"{}\",\"source\":\"{}\",\"intent\":{},\"trace\":\"{}\",\"policy\":{}}}",
                    e.t,
                    e.kind.as_str(),
                    e.source,
                    e.intent,
                    e.trace,
                    match e.policy {
                        Some(p) => format!("\"{}\"", p.as_str()),
                        None => "null".to_string(),
                    }
                )
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    /// Invariants evaluated over the log *as the selected fault leaves it*.
    ///
    /// The fault corrupts a copy of the record, never the running system —
    /// which is why the arm keeps behaving correctly while the audit trail
    /// stops being able to prove it.
    pub fn invariants(&self) -> String {
        let viewed = fault::inject(&self.log, self.fault);
        let rows: Vec<String> = invariant::check_all(&viewed, self.agent.current().map(|i| i.id))
            .into_iter()
            .map(|(inv, ok)| format!("{{\"name\":\"{}\",\"ok\":{}}}", inv.as_str(), ok))
            .collect();
        format!("[{}]", rows.join(","))
    }

    pub fn stations(&self) -> String {
        let rows: Vec<String> = STATIONS
            .iter()
            .map(|s| {
                format!(
                    "{{\"x\":{:.3},\"y\":{:.3},\"name\":\"{}\",\"sterile\":{}}}",
                    s.x, s.y, s.name, s.sterile
                )
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    pub fn faults() -> String {
        let rows: Vec<String> = fault::ALL
            .into_iter()
            .map(|f| {
                format!(
                    "{{\"id\":\"{}\",\"what\":\"{}\"}}",
                    f.as_str(),
                    f.describe()
                )
            })
            .collect();
        format!("[{}]", rows.join(","))
    }
}
