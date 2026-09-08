# Horizontal travel across the complete swing

The previous 3.75/5 mm/s experiments completed horizontal foot travel during
raise. Gear/pulley interference then occurred while the leg was also lifted.
The shared Rust sequence now has an opt-in `whole_swing_horizontal_motion`:
horizontal travel follows one quintic rest-to-rest profile across raise and
lower, while the vertical trajectory and support guards stay unchanged.
For the 0.38 + 0.38 s swing this halves the theoretical peak horizontal speed
for a given displacement, and places the foot halfway through its horizontal
travel at maximum height. It does not change physical poses or motor authority.

The predeclared development targets are 2.5, 3.75 and 5 mm/s with half-overlap
body movement, 50 Hz control, and paired 20/5 ms physics. Use the same selected
student network, CAD-derived scene, actuator bounds, solver tolerances, lateral
push, 24-second duration and forward-to-stop schedule as the prior studies.

The 2.5 mm/s pair uses the original 18/9 mm support posture. At each higher
speed compare four explicit geometric variants:

- Shift only the lateral feet's stance back by `4T(v - 0.0025)`.
- Shift all feet's stance back by that amount, preserving all first-cycle
  landing endpoints from the 2.5 mm/s baseline.
- Repeat each stance variant with front-leg support shifted back by an
  additional `2T(v - 0.0025)`, preserving its first-lift body position relative
  to the side-foot landing endpoints. T is the unchanged 1.38 s transfer period.

The stance adjustments are 6.9 / 13.8 mm at 3.75 / 5 mm/s. The additional
front support shifts are 3.45 / 6.9 mm. These are controller geometry choices,
not changes to CAD properties. They may clear one constraint while worsening
another; all failures must remain in the report. The previous 5 mm/s stance
trial's 0.472 N planned rear support motivates the support comparison.

Acceptance remains four or more transfers, each with at least 200 ms of
simultaneous 1 mm clearance, swing force at most 0.1 N and other-foot support
at least 1 N; no sampled internal overlap; body tilt at most 0.01 rad; final
position error at most 1 mm and heading error at most 0.005 rad; final phase idle.
The numerical screens remain 1 mm foot and 0.5 mm body at common reporting times.
The planning support screen remains 0.5 N. A 5 ms comparison is not convergence.
Nothing here establishes calibrated hardware behavior or a dynamic-gait limit.

```sh
cargo build --locked --release -p sim-runtime --example run_environment --example evaluate_lift
node examples/full-robot/whole-swing/prepare.mjs
node examples/full-robot/hybrid-speed/run.mjs runs/full-robot/learning/whole-swing examples/full-robot/whole-swing/status.json
```

Only short physical and numerical passes may proceed to sustained motion,
command changes, held-out disturbances, browser performance and further learning.
