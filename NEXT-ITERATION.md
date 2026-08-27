# Next iteration: indeterminate as an explicit outcome

Committed publicly 2026-08-27, in reply to a comment on the *Governed Motion*
article. The question, from Virendra Vaishnav: does the control plane write a
positive record of non-evaluation, or is an unavailable supervisor inferred from
the gap in the trace? His framing of why it matters is better than anything in
this repo: *a fail open looks exactly like an approval three months later.*

## Where the model stands today

Two cases, and only one of them is handled well.

**Absent input fails closed and writes a row.** An intent with no descriptor to
govern it by is refused under `MissingDescriptor`, rationale "intent carries no
descriptor to govern it by". "I could not evaluate this, because there was
nothing to evaluate against" is a decision with a policy id on it.

**Absent evaluator is inferred.** There is no `PolicyId` for the gate's own
unavailability. What catches it is `Invariant::TerminalReconstructable` — every
intent must reach a terminal event — so an intent that stops mid-flight fails
the check. That is detection by absence, and detection by absence is exactly
what the question is about.

What the design does buy: fail-open is not silent *at the actuator*. Execution
is reachable only through the gate, and the `LeakPlanner` / `LeakDriver` fault
injections exist to prove the checker catches a refused intent that shows
planner or driver activity. An action proceeding without evaluation is caught.
An evaluation that never happened, on an action that never happened, currently
looks like nothing at all.

## The change

Make indeterminacy a terminal outcome rather than a hole:

- A `PolicyId::EvaluatorUnavailable` (or a third verdict beside `Approved` and
  `Refusal` — `Indeterminate` — which may model it better, since it is not a
  policy that refused but the absence of one that could answer).
- A timeout on evaluation that resolves to that outcome rather than to nothing.
- An `EventKind` for it, terminal, so `TerminalReconstructable` is satisfied by
  a *record* instead of by the intent having quietly ended.
- A fault injection that drops the evaluator, and an invariant asserting that a
  dropped evaluator produces the record rather than a gap — the same discipline
  the other five faults already follow.

## Why it is worth doing

It separates two audit semantics that are currently conflated: a positive permit
is evidence of authorization, and the absence of one must never be readable as
approval. Today the second half rests on an invariant over the whole log; after
the change it rests on a row.
