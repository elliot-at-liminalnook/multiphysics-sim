# Gait lab: readable gait files for LLM-driven search

A gait is a small YAML file. Edit it, evaluate it, read the report, repeat.
Files convert exactly to the gait search's motion template (the same compiler,
screens, reduced model and gates as `compare_gait_search`), so nothing here is
a separate physics path. Format: `sim_domain_control::gait_script`; tool:
`crates/sim-runtime/examples/gait_lab.rs`.

`study.json` is the fast 12.5 V study (`../gait-search-fast-12v5-2026-09-25`)
with its own qualification directory, `qualification-q32/`.

## Commands (from the repository root)

```sh
cargo build --release -p sim-runtime --features bayesian,evolution --example gait_lab --example reduced_exploration
L=examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25
B=target/release/examples

# Start from any search trial
$B/gait_lab export $L/study.json <trial-dir> $L/gaits/my-gait.yaml
# Check files in seconds (no compile, no physics)
$B/gait_lab validate $L/study.json $L/gaits/*.yaml
# Score files, 8 at a time on the qualified fast model (~1 min each)
$B/gait_lab evaluate $L/study.json $L/results $L/gaits/a.yaml $L/gaits/b.yaml
# Confirm a finalist on the detailed model (~3x slower)
$B/gait_lab evaluate $L/study.json $L/results --fidelity detailed $L/gaits/best.yaml
# Joint-space pose sequences (stand, crouch, leg lifts): seconds, no physics
$B/gait_lab poses $L/study.json $L/results $L/poses/stand-crouch.yaml
# Steer a gait through timed commands (forward/sideways/turn): seconds, no physics
$B/gait_lab steer $L/study.json $L/results-steer $L/gaits/aws-6216-Bayesian-009.yaml $L/maneuvers/*.yaml
# Numeric polish: search a gait file's `tune:` ranges with Bayesian/CMA-ES
$B/gait_lab tune $L/study.json $L/gaits/my-gait.yaml $L/tune-my-gait --seeds 4201,4202 --attempts 20 --run
# ...then export any of its trials back to a gait file
$B/gait_lab export $L/tune-my-gait/comparison-config.json $L/tune-my-gait/comparison/<trial> $L/gaits/tuned.yaml
```

Each evaluation writes `results/<name>-<hash>/report.yaml`, `compiled.json`
(playable) and `evaluation.json`, and appends one line to
`results/journal.jsonl`. An identical file is not re-run. Create
`results/STOP` to cancel.

After any library source change, the reduced model must be re-qualified
(the runtime fingerprint covers every crate):

```sh
rm -rf $L/qualification-q32
$B/reduced_exploration prepare ../gait-search-fast-12v5-2026-09-25/baseline.spec.json ../gait-search-fast-12v5-2026-09-25/profile-q32.json $L/qualification-q32 --fresh
$B/reduced_exploration qualify $L/qualification-q32 $L/STOP
```

## The file

```yaml
version: 1
name: long-stride-short-stance
notes: |
  Idea and what changed. Keep a short hypothesis here.
cycle:
  period_s: 1.27        # one full cycle: every leg lifts and lands once
  travel_m: 0.22        # body travel per cycle along the walking diagonal
  heading_deg: 0        # optional: travel direction from the walking diagonal,
                        #   counterclockwise (180 backward, 90 left, -90 right);
                        #   the body does not turn, footholds are unchanged
body:
  height_offset_m: -0.023
  controls:             # optional; >= 4, evenly spaced over the cycle
    - {ry_deg: 0}       # x_m y_m z_m rx_deg ry_deg rz_deg (rotation vector,
    - {ry_deg: 2}       #   ≈ roll/pitch/yaw for small angles). The body
    - {ry_deg: 0}       #   follows a smooth B-spline within these controls,
    - {ry_deg: -2}      #   not through each one.
leg_defaults:           # used by every leg that does not set its own
  stance: 0.63          # fraction of the cycle on the ground
  swing_height_m: 0.026 # mid-swing lift
  return_ramp: 0.25     # swing speed-up/slow-down fraction (omit: rest-to-rest)
legs:                   # every leg must appear: -Y, +X, +Y, -X
  +X: {phase: 0.554}    # when stance starts, fraction of the cycle (0 to <1)
  +Y: {phase: 0.094}
  -X: {phase: 0.453, swing_height_m: 0.03, offset_m: [0.01, 0, 0]}
  -Y: {phase: 0.0}
controller:             # required; the study's motor-derived ranges apply
  governor_speed_rad_s: 2.46          # 1.40 .. 4.43
  governor_acceleration_rad_s2: 9.2   # 6.98 .. 10.62
tune:                   # optional: ranges a numeric search may vary later
  cycle.travel_m: [0.18, 0.26]
```

Walking speed is roughly `travel_m / period_s`, but only if the robot stays
upright and tracks its joints.

## Pose sequences

```yaml
version: 1
name: stand-crouch
base: stance            # joints a pose does not name: stance (default) or home (CAD 0)
poses:
  stand: {}
  crouch:
    all: {foot_servo_deg: -90}          # every leg with this joint
  lift:
    extends: crouch                     # start from another pose
    legs:
      +X: {foot_servo_deg: -100, hip_servo_deg: 15}   # per leg, wins over all
sequence:               # starts at the first pose; loops back to it
  - {pose: stand, move_s: 1.5, hold_s: 1.0}   # first move_s = the return move
  - {pose: crouch, move_s: 1.5, hold_s: 1.0}
  - {pose: lift, move_s: 1.0, hold_s: 1.5}
```

Joints per leg (-Y, +X, +Y, -X): `hip_servo_deg`, `worm_servo_deg`,
`foot_servo_deg` (the knee drive), as joint coordinates in degrees. Moves are
rest-to-rest (zero speed at every pose). Checks, per pose and between poses:
controller command range, motor speed at the study's supply, and CAD linkage
closure and authored limits. Status is `ready` or `blocked` with reasons.
Pose sequences are not simulated with physics (the walking controller is built
for contact-phase gaits), so balance is not checked; play them with the leg
suspended.

## Maneuvers (`steer`)

A maneuver steers a gait file with timed body commands. Forward is the gait's
walking direction, lateral is to its left, yaw is counterclockwise from above.

```yaml
version: 1
name: tour
limits:                 # command bounds and how fast commands may change
  forward_m_s: 0.3
  lateral_m_s: 0.25
  yaw_deg_s: 25
  forward_m_s2: 0.15
  lateral_m_s2: 0.15
  yaw_deg_s2: 20
start_phase: 0          # optional: where in the gait cycle to begin (0 to <1)
commands:               # each holds until the next at_s; the first is at 0
  - {at_s: 0}                               # step in place
  - {at_s: 1, forward_m_s: 0.25}
  - {at_s: 7, forward_m_s: 0.15, yaw_deg_s: 12}   # arc left
  - {at_s: 13, yaw_deg_s: -20}              # turn right in place
  - {at_s: 19, lateral_m_s: 0.15}           # sideways left
  - {at_s: 25, forward_m_s: -0.15}          # backward
  - {at_s: 31}                              # stop: step in place
duration_s: 35
```

The steered gait (`sim_domain_control::contact_phase::steered`) keeps the
gait's timing, swing shape, lift and body motion. It moves the body along a
planned path and places each foothold at the gait's own foothold, carried by
the body path at that step's mid-stance. The path is fixed one commitment
horizon ahead (about 0.7 of a cycle, the longest time from a liftoff to the
next mid-stance), so a foothold is final before its foot lifts. Stance feet
never slide, and a command reaches the body after that delay, rate limited.
A constant command equal to the gait's own travel reproduces the gait
exactly (tested). Commanding zero steps in place; the gait clock does not
stop.

`steer` places every 10 ms of the maneuver through the study's CAD model
(the same IK the contact planner uses). It checks that each reference is
reachable, inside the controller's command range, within motor speed and
inside the CAD limits. It reports internal link overlap next to the gait's
own straight walk through the same placement. Output: `report.yaml`
(status, summary, joints by % of motor speed, travel, turn, overlap) and
`trace.json` (joint angles, body path, feet). Kinematics only: no loads,
contact or balance. A constant-heading pattern (backward, sideways) can be
simulated as a gait file with `heading_deg` using `evaluate`. Commanded
transitions and turning need the online `steered_gait` policy reference
(`sim_runtime::steered_reference`), which has no physics qualification yet.

Results 2026-09-27 (gait `aws-6216-Bayesian-009`, fast model, 3.6 s, one seed):
forward 0.200 m/s, `6216-backward` 0.161, `6216-sideways-left` 0.153,
`6216-sideways-right` 0.132. All passed the upright and tracking gates, with
the tightest joint at 99–100% of motor speed. `steer` on `maneuvers/`: every
file is reachable within motor speed (the tour peaks at 89% on the -X worm).
The gait's own straight walk shows 0.314 mm internal link overlap through this
placement, above the planner's 0.1 mm tolerance; `forward-start-stop` peaks
at 0.330 mm and is marked blocked.

## Numeric polish (`tune`)

`gait_lab tune` writes a new study directory from a gait file: only the
numbers named in `tune:` are searched, starting from the file's values;
controller values it does not tune become fixed controller parameters. Its
`run.sh` qualifies the reduced model on this gait's own baseline, then runs
`compare_gait_search` (two streams per seed). `gait-lab.json` records the
source file and the fixed values, so exported trials evaluate here again.

## In the calibration panel

Gaits that passed here (with `spec-identity.json`, written since 2026-09-25)
and `ready` pose sequences appear in the panel's gait playback list after the
passed search trials, labelled `gait-lab-…` and `Poses · …`. The server must
be rebuilt and restarted to pick up new UI code, not new results.

## Reading a report

- `status`: `passed`, `rejected` (simulated, failed a gate), `screened_out`
  (never simulated: kinematics, joint speed, schedule or leg tracking), or
  `invalid` (file problem; the reason names the path).
- `speed_m_s`: eligible forward speed; the objective.
- `gates`: body upright (`body_up_z_minimum` ≥ 0.9), worst joint tracking
  RMS ≤ 5° and peak ≤ 15°.
- `joints`: sorted by how close the planned motion comes to the motor's speed
  at 12.5 V (`percent_of_limit`); the top joint is usually what limits a
  faster gait. Joint speed above the limit is screened out before physics.
- `reasons`: why it failed, including early stops ("stopped at 1.50 s: …").

## Scope

Fast results use the reduced model, qualified at task level (same outcome,
speed score within 25 % of the detailed model on the baseline). Confirm any
finalist with `--fidelity detailed` before playing it on the leg. Simulated
walking is not a claim about the physical robot.
