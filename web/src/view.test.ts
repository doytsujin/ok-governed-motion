import { describe, expect, it } from "vitest";
import { classFor, eventLabel, isPastGate, PHASES, project, toPx } from "./view";

describe("projection", () => {
  it("puts the arm base on the bottom edge, not the centre", () => {
    const p = project(720, 470, 0.85);
    const [, y] = toPx(p, 0, 0);
    expect(y).toBeGreaterThan(470 * 0.8);
  });

  it("keeps the whole reachable arc inside the canvas", () => {
    const w = 720;
    const h = 470;
    const reach = 0.85;
    const p = project(w, h, reach);
    // Sample the half-disc the arm can reach and assert nothing escapes.
    for (let a = 0; a <= Math.PI; a += Math.PI / 24) {
      const [x, y] = toPx(p, reach * Math.cos(a), reach * Math.sin(a));
      expect(x).toBeGreaterThanOrEqual(0);
      expect(x).toBeLessThanOrEqual(w);
      expect(y).toBeGreaterThanOrEqual(0);
      expect(y).toBeLessThanOrEqual(h);
    }
  });
});

describe("gate", () => {
  it("marks exactly the two phases with physical side effects", () => {
    expect(PHASES.filter(isPastGate)).toEqual(["PLANNING", "EXECUTING"]);
  });

  it("does not treat reasoning as past the gate", () => {
    // If this ever flips, the diagram would imply policy runs after planning.
    expect(isPastGate("REASONING")).toBe(false);
    expect(isPastGate("IDLE")).toBe(false);
  });
});

describe("log rows", () => {
  it("renders state events as transitions", () => {
    expect(eventLabel("STATE_EXECUTING")).toBe("→ EXECUTING");
    expect(eventLabel("POLICY_REFUSE")).toBe("POLICY_REFUSE");
  });

  it("colours refusals and completions differently", () => {
    expect(classFor("POLICY_REFUSE")).toBe("refuse");
    expect(classFor("INTENT_REFUSED")).toBe("refuse");
    expect(classFor("INTENT_COMPLETE")).toBe("ok");
    expect(classFor("TELEMETRY")).toBe("");
  });
});
