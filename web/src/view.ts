/**
 * Pure view helpers — no DOM, no wasm, so they can be tested directly.
 */

export interface Station {
  x: number;
  y: number;
  name: string;
  sterile: boolean;
}

/** World units are metres-ish and y is up; canvas pixels are the other way. */
export interface Projection {
  scale: number;
  ox: number;
  oy: number;
}

/**
 * Fit the arm's reachable area into the canvas with a margin.
 *
 * The base sits on the bottom edge rather than the centre: the workspace is a
 * half-disc above it, and centring would waste half the canvas on space the
 * arm can never occupy.
 */
export function project(w: number, h: number, reach: number): Projection {
  const margin = 26;
  const scale = Math.min((w - margin * 2) / (reach * 2), (h - margin * 2) / reach);
  return { scale, ox: w / 2, oy: h - margin };
}

export function toPx(p: Projection, x: number, y: number): [number, number] {
  return [p.ox + x * p.scale, p.oy - y * p.scale];
}

export const PHASES = ["IDLE", "REASONING", "PLANNING", "EXECUTING", "PUBLISHING"] as const;
export type Phase = (typeof PHASES)[number];

/** Phases that have physical side effects — everything past the gate. */
export function isPastGate(phase: string): boolean {
  return phase === "PLANNING" || phase === "EXECUTING";
}

/** Short label for an event row. */
export function eventLabel(kind: string): string {
  return kind.replace(/^STATE_/, "→ ");
}

export function classFor(kind: string): string {
  if (kind === "POLICY_REFUSE" || kind === "INTENT_REFUSED") return "refuse";
  if (kind === "POLICY_OK" || kind === "INTENT_COMPLETE") return "ok";
  return "";
}
