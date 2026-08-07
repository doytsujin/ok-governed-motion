# Governed Motion

A manipulator that cannot be commanded without a policy decision, and a page
that lets you watch it refuse.

An intent is evaluated against policy before any planner or driver is touched.
Refusal is a property of the type system rather than of careful coding: the
planner and driver entry points require an approval token that only the policy
evaluator can construct, so actuating on a refused intent is a program that does
not compile.

```
Idle → Reasoning → Planning → Executing → Publishing → Idle
```

A refusal returns to Idle without entering Planning or Executing.

## Layout

| Path | |
| --- | --- |
| `crates/gov` | Policy evaluation and the approval token |
| `crates/robot` | Lifecycle, planner, driver, kinematics |
| `crates/wasm` | Browser bindings |
| `web/` | The page |
| `scripts/drive.mjs` | Headless behavioural check |

## Running

```sh
cargo test              # 14 tests: refusal, fault discrimination, IK
cd web && npm install
npm run dev             # rebuilds the wasm first; needs wasm-pack
npm test                # 6 view tests
```

The headless check loads the built page, ticks the operator-present control, and
asserts the page reports refusals and no completions during that window:

```sh
python3 -m http.server 8791 --bind 127.0.0.1 -d web/dist &
google-chrome --headless=new --remote-debugging-port=9222 about:blank &
node scripts/drive.mjs http://127.0.0.1:8791/ shot.png
```

## Scope

A simulation with no physics engine and no hardware. Policy is evaluated against
ground-truth state supplied by the simulation, so nothing here measures a real
manipulator or a real control loop, and no figure it shows is a hardware
measurement.

Apache-2.0.
