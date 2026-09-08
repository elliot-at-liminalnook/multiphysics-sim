# Direct transfer between support postures

The slower-shift ablation reduces the contact-motion ratio from 93% to 29%,
but costs travel speed and still fails the 5% screen. The shared Rust direct
transfer option now holds the end-of-swing body reference after landing and
shifts directly to the next support posture. It eliminates the separate body
return through center, retaining readiness guards and explicit stop recentering.

Predeclare three sequential cases before seeing robot outcomes:

1. Repeat the original 24-second integral-teacher steering case with the rebuilt
   runtime and the new option absent. Require exact physical frames, task
   transitions and recording against the retained native result, excluding
   only the frame's cumulative wall-clock field.
2. A 60-second forward/stop direct-transfer case with phase durations
   **[0.58, 0.38, 0.38, 0.02, 0.02] seconds**. The total nominal transfer stays
   **1.38 seconds**, forward command **3.75 mm/s**, and nominal stride **5.175 mm**.
3. The original 24-second forward/turn/reverse/stop inputs with the same new
   direct-transfer configuration.

Keep the CAD robot, controller gains/integral, contact model, force coefficients,
motor limits, 1.25 ms physics and solver tolerances fixed. Only the shared
sequence option and the explicit phase-time allocation change. Existing
development pushes and inputs remain. Recenter now uses shift plus settle time;
the two short post-landing phases remain real guarded controller samples.

Preserve every runtime failure and task result. The original swing, tilt,
heading, stopping, sampled-collision and support requirements still apply.
Evaluate the minute against the previously declared maximum per-foot integrated
load-weighted contact motion / net body advance limit of **5%**. Verify that
planned landing foot positions and order match the original transfer sequence;
the body path intentionally differs. Report actual travel, work, contact motion
by phase and native compute. Do not infer timestep, fresh held-out, terrain or
browser qualification. The legacy live WASM bundle lacks this new option and
must remain intact; a new isolated build is required before browser promotion.
