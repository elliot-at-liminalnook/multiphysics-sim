# Direct measured diagonal speed continuation

The user redirected work from fixed-speed offline restoration to increasing
measured robot speed. Five completed Rust runtime experiments reproduce the
.21 diagonal baseline, increase cadence, and refine the .23 case timestep.
The browser and its original bundle remain unchanged.

| Command m/s | Physics step ms | Forward / reverse m/s | Loaded slip | Passed lifts | Maximum sampled overlap µm |
| --- | ---: | --- | ---: | --- | ---: |
| .211710 | .625 | .210057 / .206958 | 7.73% | 38/38 | 12.143 |
| .230 | .625 | .229225 / .226941 | 9.04% | 41/42 | 12.374 |
| .250 | .625 | .254952 / .250969 | 14.51% | 40/46 | 12.869 |
| .280 | .625 | .293549 / .289339 | 23.27% | 38/50 | 13.884 |
| .230 | .3125 | .228988 / .226466 | 9.11% | 41/42 | 12.412 |

All complete eight seconds and pass the existing short speed, heading, tilt
and stopping checks. None passes all contact-quality, lift and geometry gates.
The .28 command barely meets speed tracking and has substantial sliding: it
is faster experimental motion, not a qualified gait. The .23 result is the
more useful next development point. Its .625/.3125 ms chassis paths differ
by at most 0.535 mm, and forward speed differs by 0.104%. This is one refinement
comparison, not full convergence. No sustained, yaw-control, command-loss,
browser performance or hardware-transfer validation has been added.

Each case uses identical CAD, world, friction, actuator and joint limits,
initial pose, reference curves, controller code, command times and seed zero.
Only the requested speed and its input bounds change, plus the explicit
timestep refinement in the last case. Existing failed inverse-reference audits
remain visible. Shared Rust computes the trajectory, contact and dynamics;
Node only assembles inputs and reduces measurements. Actual motor outputs
remain subject to the original torque-speed model.

For a fixed joint path with clock rate r, reference velocity scales with r
and acceleration with r². The existing Rhai controller already uses these
rate terms and static/odd/even feedforward; off-nominal and transient load
compensation remains approximate. The .23/.25/.28 rates are 1.0864/1.1809/1.3226
times the original reference. The new shared Rust capability calculation
finds a conditional no-load rate budget of **.212496 m/s**, first limited by
the -X worm coordinate, with -Y almost tied. Belt-coordinate budgets range
from .333 to .397 m/s on this same path. These are exact reference extrema,
not attainable loaded speeds or a global robot maximum. Actual faster motion
can depart from the path and slide; the new measurements do exactly that.

The next speed work should reduce worm demand by changing the path to share
more motion with the belts, then measure that candidate directly in the
runtime. A higher cadence alone produces worsening contact behavior. Preserve
the current physical gates and investigate the sampled overlaps; do not treat
their small size as permission to ignore them. The contact-implicit grid
experiment remains archived as unfinished research and has no runtime speed
gain to claim.

Reproduce a case from the repository root:

```sh
node examples/full-robot/contact-planning/prepare_cadence_screen.mjs \
  runs/contact-planning/diagonal21-screen fresh-cadence-name 0.23 0.0003125
# Run the generated scene/config/actions with the existing run_environment
# example and examples/full-robot/fast-wasd/task.json.
node examples/full-robot/contact-planning/analyze_controller_screen.mjs \
  runs/contact-planning/fresh-cadence-name fresh-summary.json 0.23
node examples/full-robot/contact-planning/audit_contact_controller.mjs \
  examples/full-robot/contact-planning/fresh-cadence-name-trial.json fresh-clearance.json
```

`summarize_cadence_speed.mjs` reproduces `cadence-speed-summary.json` and checks
matched inputs, normalized runtime configuration, command schedules, capture
hashes and exact policy replay. The analyzer's optional comparison speed is
checked against recorded commands; its default reproduces the original .21
summary byte for byte, and a deliberately wrong comparison command is rejected.
The current runtime reproduces the original .21 speed and slip numbers.
No Rust physics or controller source was changed in this continuation.

Raw inputs, captures, replay states and geometry audits are durably preserved
in `cadence-speed-evidence.tar.gz` with per-file and executable identities in
`cadence-speed-evidence-index.json`. The separate CAD baseline is referenced
by its content hash. Extract the archive at the repository root to restore
its recorded relative paths.
