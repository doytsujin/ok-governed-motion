/**
 * Drives the wasm simulation and draws it.
 *
 * Canvas2D rather than WebGL, deliberately: the scene is a four-station
 * workcell and a two-link arm. A GPU pipeline here would be borrowed
 * machinery, not engineering.
 */

import init, { Sim } from "../pkg/govmotion.js";
import {
  classFor,
  eventLabel,
  isPastGate,
  PHASES,
  project,
  toPx,
  type Projection,
  type Station,
} from "./view";

const REACH = 0.85; // L1 + L2 from the robot crate

/**
 * Canvas colours come from the stylesheet rather than from literals here, so
 * the two themes have a single source of truth and the arm cannot end up dark
 * on a light page. Read once, and again whenever the theme changes.
 */
function readColors() {
  const cs = getComputedStyle(document.documentElement);
  const v = (n: string) => cs.getPropertyValue(n).trim();
  return {
    reachFill: v("--c-reach-fill"),
    reachLine: v("--c-reach-line"),
    sterileFill: v("--c-sterile-fill"),
    sterileFillLocked: v("--c-sterile-fill-locked"),
    sterileLine: v("--c-sterile-line"),
    stationFill: v("--c-station-fill"),
    stationLine: v("--c-station-line"),
    arm: v("--c-arm"),
    jointFill: v("--c-joint-fill"),
    jointLine: v("--c-joint-line"),
    gripEmpty: v("--c-grip-empty"),
    accent: v("--accent"),
    ok: v("--ok"),
    refuse: v("--refuse"),
    dim: v("--dim"),
  };
}

let C = readColors();
addEventListener("themechange", () => {
  C = readColors();
});

interface Snapshot {
  t: number;
  state: string;
  elbow: [number, number];
  tip: [number, number];
  held: boolean;
  plates: boolean[];
  completed: number;
  refused: number;
  queued: number;
  human: boolean;
  sterileLocked: boolean;
  maxDwell: number;
  current: { id: number; source: number; dest: number; dwell: number };
  last:
    | { none: true }
    | { approved: true; intent: number }
    | { approved: false; intent: number; policy: string; why: string };
}

interface EventRow {
  t: number;
  kind: string;
  source: string;
  intent: number;
  trace: string;
  policy: string | null;
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

async function main() {
  await init();
  const sim = new Sim(42);
  const stations: Station[] = JSON.parse(sim.stations());
  const faults: { id: string; what: string }[] = JSON.parse(Sim.faults());

  const canvas = $<HTMLCanvasElement>("cell");
  const ctx = canvas.getContext("2d")!;

  // -- controls ----------------------------------------------------------
  const human = $<HTMLInputElement>("human");
  const sterile = $<HTMLInputElement>("sterile");
  const dwell = $<HTMLInputElement>("dwell");
  const auto = $<HTMLInputElement>("auto");
  const faultSel = $<HTMLSelectElement>("fault");

  human.onchange = () => sim.set_human(human.checked);
  sterile.onchange = () => sim.set_sterile_locked(sterile.checked);
  auto.onchange = () => sim.set_auto(auto.checked);
  dwell.oninput = () => {
    const v = parseFloat(dwell.value);
    sim.set_max_dwell(v);
    $("dwellv").textContent = `${v.toFixed(1)} s`;
  };

  for (const f of faults) {
    const o = document.createElement("option");
    o.value = f.id;
    o.textContent = f.id;
    faultSel.append(o);
  }
  faultSel.onchange = () => {
    sim.set_fault(faultSel.value);
    const f = faults.find((x) => x.id === faultSel.value);
    $("faultwhat").textContent = f
      ? f.id === "none"
        ? "Unperturbed. Every invariant should hold."
        : `Injected: ${f.what}. Exactly one invariant should drop — the arm keeps behaving correctly, but the record can no longer prove it.`
      : "";
  };

  // -- FSM nodes ---------------------------------------------------------
  const fsm = $("fsm");
  const nodes = PHASES.map((p) => {
    const el = document.createElement("div");
    el.className = "node" + (isPastGate(p) ? " gate" : "");
    el.textContent = p;
    fsm.append(el);
    return el;
  });

  // -- draw --------------------------------------------------------------
  function drawCell(s: Snapshot, p: Projection) {
    const { width: w, height: h } = canvas;
    ctx.clearRect(0, 0, w, h);

    // Reachable workspace.
    ctx.beginPath();
    ctx.arc(p.ox, p.oy, REACH * p.scale, Math.PI, 2 * Math.PI);
    ctx.fillStyle = C.reachFill;
    ctx.fill();
    ctx.strokeStyle = C.reachLine;
    ctx.stroke();

    // Sterile zone: a band around the stations marked sterile.
    for (const st of stations) {
      if (!st.sterile) continue;
      const [x, y] = toPx(p, st.x, st.y);
      ctx.beginPath();
      ctx.arc(x, y, 0.19 * p.scale, 0, Math.PI * 2);
      ctx.fillStyle = s.sterileLocked ? C.sterileFillLocked : C.sterileFill;
      ctx.fill();
      ctx.strokeStyle = s.sterileLocked ? C.refuse : C.sterileLine;
      ctx.setLineDash([4, 3]);
      ctx.stroke();
      ctx.setLineDash([]);
      if (s.sterileLocked) {
        ctx.fillStyle = C.refuse;
        ctx.font = "11px ui-monospace, monospace";
        ctx.textAlign = "center";
        ctx.fillText("LOCKED", x, y - 0.19 * p.scale - 7);
      }
    }

    // Stations.
    stations.forEach((st, i) => {
      const [x, y] = toPx(p, st.x, st.y);
      ctx.beginPath();
      ctx.roundRect(x - 26, y - 15, 52, 30, 4);
      ctx.fillStyle = C.stationFill;
      ctx.fill();
      ctx.strokeStyle = i === s.current.dest ? C.accent : C.stationLine;
      ctx.stroke();
      if (s.plates[i]) {
        ctx.beginPath();
        ctx.arc(x, y, 8, 0, Math.PI * 2);
        ctx.fillStyle = C.ok;
        ctx.fill();
      }
      ctx.fillStyle = C.dim;
      ctx.font = "11px ui-sans-serif, system-ui";
      ctx.textAlign = "center";
      ctx.fillText(st.name, x, y + 28);
    });

    // Arm.
    const [bx, by] = toPx(p, 0, 0);
    const [ex, ey] = toPx(p, s.elbow[0], s.elbow[1]);
    const [tx, ty] = toPx(p, s.tip[0], s.tip[1]);
    const moving = s.state === "EXECUTING";
    ctx.lineCap = "round";
    ctx.strokeStyle = moving ? C.accent : C.arm;
    ctx.lineWidth = 9;
    ctx.beginPath();
    ctx.moveTo(bx, by);
    ctx.lineTo(ex, ey);
    ctx.lineTo(tx, ty);
    ctx.stroke();
    for (const [jx, jy, r] of [
      [bx, by, 8],
      [ex, ey, 6],
    ] as const) {
      ctx.beginPath();
      ctx.arc(jx, jy, r, 0, Math.PI * 2);
      ctx.fillStyle = C.jointFill;
      ctx.fill();
      ctx.strokeStyle = C.jointLine;
      ctx.lineWidth = 2;
      ctx.stroke();
    }
    // Gripper, carrying a plate or not.
    ctx.beginPath();
    ctx.arc(tx, ty, 9, 0, Math.PI * 2);
    ctx.fillStyle = s.held ? C.ok : C.gripEmpty;
    ctx.fill();
    ctx.strokeStyle = C.arm;
    ctx.lineWidth = 2;
    ctx.stroke();

    // Operator in the cell — the reason a refusal is about to happen.
    if (s.human) {
      const [hx, hy] = toPx(p, -0.62, 0.05);
      ctx.fillStyle = C.refuse;
      ctx.beginPath();
      ctx.arc(hx, hy - 16, 7, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillRect(hx - 6, hy - 7, 12, 20);
      ctx.font = "11px ui-monospace, monospace";
      ctx.textAlign = "center";
      ctx.fillText("operator", hx, hy + 28);
    }
  }

  function drawDecision(s: Snapshot) {
    const el = $("decision");
    if ("none" in s.last) {
      el.className = "decision";
      el.textContent = "no decision yet";
      return;
    }
    if (s.last.approved) {
      el.className = "decision ok";
      el.innerHTML = `<b>approved</b> — intent ${s.last.intent} cleared all five policies`;
    } else {
      el.className = "decision no";
      el.innerHTML =
        `<b>refused</b> — intent ${s.last.intent} · ` +
        `<span class="pill">${s.last.policy}</span><br>${s.last.why}`;
    }
  }

  function drawInvariants() {
    const rows: { name: string; ok: boolean }[] = JSON.parse(sim.invariants());
    $("invs").innerHTML = rows
      .map(
        (r) =>
          `<div class="inv"><span>${r.name}</span>` +
          `<span class="v ${r.ok ? "pass" : "fail"}">${r.ok ? "holds" : "DROPPED"}</span></div>`
      )
      .join("");
  }

  function drawLog() {
    const rows: EventRow[] = JSON.parse(sim.events(40));
    $("logtab").innerHTML = rows
      .slice()
      .reverse()
      .map(
        (e) =>
          `<tr class="${classFor(e.kind)}">` +
          `<td>${e.t.toFixed(2)}</td>` +
          `<td>#${e.intent}</td>` +
          `<td class="trace">${e.trace || "&lt;no trace&gt;"}</td>` +
          `<td>${eventLabel(e.kind)}</td>` +
          `<td class="trace">${e.policy ?? ""}</td></tr>`
      )
      .join("");
  }

  // -- loop --------------------------------------------------------------
  let last = performance.now();
  let logAccum = 0;

  function frame(now: number) {
    const dt = Math.min((now - last) / 1000, 0.1);
    last = now;
    sim.tick(dt);

    const s: Snapshot = JSON.parse(sim.snapshot());
    const p = project(canvas.width, canvas.height, REACH);
    drawCell(s, p);
    drawDecision(s);

    nodes.forEach((n, i) => n.classList.toggle("on", PHASES[i] === s.state));
    $("s-done").textContent = String(s.completed);
    $("s-ref").textContent = String(s.refused);
    $("s-q").textContent = String(s.queued);
    $("s-t").textContent = s.t.toFixed(1);

    // The log and invariant panels do not need 60 Hz, and rebuilding them
    // every frame makes the trace unreadable as it scrolls.
    logAccum += dt;
    if (logAccum > 0.25) {
      logAccum = 0;
      drawLog();
      drawInvariants();
    }
    requestAnimationFrame(frame);
  }

  faultSel.onchange!(new Event("change"));
  drawInvariants();
  requestAnimationFrame(frame);
}

main().catch((err) => {
  document.body.innerHTML =
    `<pre style="padding:22px;color:#f85149">failed to start: ${err}</pre>`;
});
