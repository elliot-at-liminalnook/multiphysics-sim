# Faster student accuracy and browser profile

The first fitted student passes the 1.25 ms mixed-steering task, improving
final body error from 1.370 to 0.787 mm relative to its unfitted network.
Minute-long and held-out closed-loop tests are still running when this plan
is declared. Do not choose a different student checkpoint using those outcomes.

Keep the selected epoch and physical CAD definition fixed. If the minute
completes, compare it with a 0.625 ms run at the same 50 Hz reporting and input
times, even if its endpoint task fails. Use the existing 1 mm foot / 0.5 mm
body whole-trajectory screens and original task acceptance.

For browser evaluation, explicitly copy the previously measured numerical
solver profile (bounded Broyden updates, tangent probes at 1e-5 and exact-base
reuse) into this student configuration. Preserve all Newton/contact-history
tolerances and physical options unless an existing profile difference is
recorded explicitly. Evaluate 20 ms minute and mixed steering, plus a 5 ms
minute reference. Compare 20/5 ms with the same unchanged accuracy screens.
All failures remain artifacts; no endpoint or numerical threshold is adjusted.

For the 20 ms steering case, compare feedback calculations enabled/disabled
before omitting unused body/point suggestions and policy-side floor-force
observations in browser timing. Require strict physical-frame and transition
identity using the existing shared omission checker. The planner and physical
contact retain their own required observations.

If the browser candidate completes, package its exact recipe as an
experimental leaderboard entry. Require full native/WASM fixed-tolerance
parity, replay/reset, exact Load and run, and sequential rendered forward and
steering timing against the existing >=1 simulation/wall and <=20 ms active
p95 requirements. A failed held-out case or refinement screen keeps it out of
validated speed rankings. This is a faster learned motor policy with a still
privileged planner, not calibrated sim-to-real control.
