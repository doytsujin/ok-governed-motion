#!/usr/bin/env node
// Drive the page in a real browser and assert what it actually did.
//
// A screenshot proves it rendered. This proves it *behaves*: it toggles the
// operator-present control, waits, and checks the page reports refusals and no
// completions during that window — which is the whole claim the page makes.
//
//   node scripts/drive.mjs <url> [out.png]
//
// Expects a headless Chrome on :9222 and a server already serving the build.
// Node's built-in WebSocket does the CDP talking, so there is no dependency.

import { writeFileSync } from "node:fs";

const [url, out] = process.argv.slice(2);
if (!url) {
  console.error("usage: drive.mjs <url> [out.png]");
  process.exit(2);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function target() {
  for (let i = 0; i < 60; i++) {
    try {
      const list = await (await fetch("http://127.0.0.1:9222/json")).json();
      const page = list.find((t) => t.type === "page" && t.webSocketDebuggerUrl);
      if (page) return page;
    } catch {
      /* Chrome is still coming up. */
    }
    await sleep(200);
  }
  throw new Error("no debuggable page appeared on :9222");
}

const page = await target();
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));

let id = 0;
const pending = new Map();
ws.onmessage = (m) => {
  const msg = JSON.parse(m.data);
  const p = pending.get(msg.id);
  if (p) {
    pending.delete(msg.id);
    msg.error ? p.reject(new Error(msg.error.message)) : p.resolve(msg.result);
  }
};
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const n = ++id;
    pending.set(n, { resolve, reject });
    ws.send(JSON.stringify({ id: n, method, params }));
  });

const evaluate = async (expr) => {
  const r = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.text);
  return r.result.value;
};

// Surface page errors instead of silently screenshotting a blank canvas.
const errors = [];
await send("Runtime.enable");
await send("Log.enable");
ws.addEventListener("message", (m) => {
  const msg = JSON.parse(m.data);
  if (msg.method === "Runtime.exceptionThrown") {
    errors.push(msg.params.exceptionDetails.text);
  }
  if (msg.method === "Log.entryAdded" && msg.params.entry.level === "error") {
    errors.push(msg.params.entry.text);
  }
});

await send("Page.enable");
await send("Page.navigate", { url });
await sleep(1200);

const read = () =>
  evaluate(`(() => ({
    done: +document.getElementById('s-done').textContent,
    refused: +document.getElementById('s-ref').textContent,
    t: parseFloat(document.getElementById('s-t').textContent),
    state: [...document.querySelectorAll('#fsm .node')].find(n => n.classList.contains('on'))?.textContent ?? '',
    decision: document.getElementById('decision').textContent.trim().replace(/\\s+/g, ' '),
    rows: document.querySelectorAll('#logtab tr').length,
    invs: [...document.querySelectorAll('.inv')].map(e => e.textContent.trim().replace(/\\s+/g,' ')),
  }))()`);

const fail = (m) => {
  console.error("FAIL " + m);
  process.exitCode = 1;
};

// 1. It runs at all.
await sleep(4000);
const running = await read();
console.log("running:", JSON.stringify(running, null, 1));
if (!(running.t > 1)) fail("simulation time is not advancing");
if (running.done < 1) fail("no intent completed in a clean environment");
if (running.rows < 5) fail("trace panel is empty");

// 2. An operator in the cell refuses everything, and nothing completes.
await evaluate(
  "(() => { const c = document.getElementById('human'); c.checked = true; c.onchange(); })()"
);
const before = await read();
await sleep(5000);
const blocked = await read();
console.log("with operator present:", JSON.stringify(blocked, null, 1));
if (blocked.refused <= before.refused) fail("no refusal while the operator was present");
if (blocked.done !== before.done) fail("an intent completed while the operator was present");
if (!/HUMAN_LOCKOUT/.test(blocked.decision)) fail("decision does not name HUMAN_LOCKOUT");

// 3. A fault drops exactly one invariant.
await evaluate(
  "(() => { const s = document.getElementById('fault'); s.value = 'leak-driver'; s.onchange(); })()"
);
await sleep(700);
const faulted = await read();
const dropped = faulted.invs.filter((i) => /DROPPED/.test(i));
console.log("leak-driver dropped:", dropped);
if (dropped.length !== 1) fail(`expected exactly 1 dropped invariant, got ${dropped.length}`);
if (!/no actuation after a refusal/.test(dropped[0] ?? "")) {
  fail("leak-driver dropped the wrong invariant");
}

if (errors.length) fail("page errors: " + errors.join(" | "));

if (out) {
  const shot = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(out, Buffer.from(shot.data, "base64"));
  console.log("wrote " + out);
}
console.log(process.exitCode ? "FAILED" : "OK — all checks passed");
ws.close();
