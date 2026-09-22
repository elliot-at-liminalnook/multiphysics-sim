# Quadruped reduced exploration qualification

This directory tests an existing gait under paired runtime profiles. No new
gait candidate or optimization step is generated. The original detailed CAD,
actuator properties, controller, terrain and initial state are preserved.

`protocol.json` records the initial, pre-measurement cases and budgets. The
source is the previously selected `upright-fast/10s/spec.json`. The test cases
use its first 3.6 seconds (four nominal gait cycles), with stop/reverse commands
introduced at 2 seconds. The initial cases preserve the original policy bounds.
The later `exploration-*.spec.json` cases explicitly allow 0.95–1.05 pace and
amplitude scales, with baseline values unchanged at 1.0. No point in this
neighborhood was sampled by an optimizer, and it is not a certified envelope.

The supplied voltage is still 11.1 V, temperature is imposed, and motor-family
parameters remain provisional. Agreement here does not qualify battery sag,
loaded hardware, rough terrain, longer episodes or arbitrary new gaits.

## Qualified mode

`reduced-exploration-outer-broyden.profile.json` passed all three cases with
unchanged budgets. It retains full articulated geometry, every detailed motor
state, the original 1/6400 s physics step and 20 ms task-input clock. It reduces
mechanism derivative work using local probes and outer-only Broyden updates;
accepted physical states still undergo the shared solver's exact residual checks.

| Case (3.6 simulated seconds) | Stepping speedup | Result |
| --- | ---: | --- |
| `outer-forward/` | 1.159x | Passed |
| `outer-stop/` | 1.166x | Passed |
| `outer-reverse/` | 1.156x | Passed |

Across these cases, the maximum joint-angle difference was **0.172 degrees**,
link-position difference **0.492 mm**, contact-force difference **3.540 N**,
current difference **0.096 A** and driver-voltage difference **0.532 V**.
Joint-rate error (0.481 rad/s against 0.5) and heating error (0.957 W against
1.0) were close to their budgets in the forward case. Longer/new motions need
their own checks. Stop and reverse differed only near numerical precision.
These numbers describe agreement with the detailed simulator, not hardware.

`status.json` summarizes the actual receipts and capture hashes. Each
`outer-*/qualification.json` retains all per-channel errors, runtime identity,
exact configuration changes and timing provenance. The final source is in
`outer-broyden-library-source.tar.gz`; `outer-forward/qualifier.bin` retains
the executable. This is reduced solver cost, not a reduced physical-state model.

The case is ready for explicit future `search_motion init-reduced` use with
`outer-forward/` and `search-settings.json`; no initialization, optimizer step
or gait search was run as part of this work.

## Retained rejected profiles

`forward/` contains the first winding-only, twice-timestep comparison.
Its detailed run and reduced run share the same executable/source identity.
The reduced run was 1.20895x faster but **failed** the original budgets:
peak contact-force difference 52.95 N; peak joint-rate difference 5.85 rad/s;
peak driver-voltage difference 4.06 V. Position agreement alone looked much
better (3.42 mm maximum link-position difference), which is why the additional
physical checks matter. This profile was not promoted, and no tolerance was
changed. `forward/qualifier.bin` preserves the executable that produced it.

The conservative follow-up keeps full motor dynamics and uses twice the
physics timestep, with the same acceptance budgets. Its cases use separate
`conservative-*` directories. The shared library source for this follow-up is
retained in `library-source.tar.gz`; captures also include full runtime identity,
CAD input, controller source, held action events and seed.

| Forward-case profile | Stepping speedup | Result |
| --- | ---: | --- |
| Winding storage omitted, twice timestep | 1.209x | Failed physical/electrical agreement |
| Full motors, twice timestep | 1.213x | Failed physical/electrical agreement |
| Local mechanism derivative probes | 1.072x | Accuracy passed; below 1.1x speed gate |
| Mechanism probes plus velocity prediction | 1.045x | Accuracy passed; below speed gate |
| Broyden updates inherited by motor inner solves | 0.731x | Failed accuracy and speed |

The other cases for these rejected profiles are prepared only. Rejected
profiles were not promoted, and the predeclared budgets were never relaxed.
The initial Broyden experiment identified avoidable work in the motor inner
solves. The final shared solver option separates outer geometry updates from
inner motor derivative policy; all physical states and acceptance tolerances
remain unchanged.

Each executed profile retains its captures and qualification receipt. Source
archives named `mechanism-library-source.tar.gz`,
`predictive-library-source.tar.gz`, `broyden-library-source.tar.gz` and
`outer-broyden-library-source.tar.gz` retain the successive library versions.
Old `qualifier.bin` executables in the rejected forward directories reproduce
the exact historical behavior; current code deliberately uses outer-only
Broyden for new exploration recipes.

## Validation scope

Every sampled mechanical/electrical channel must stay within its original
budget: angle 1 degree, link position 1 cm, angular speed 0.5 rad/s, linear
speed 0.1 m/s, contact force 10 N, torque 0.25 Nm, current 0.2 A, voltage
0.75 V and power/heating 1 W. All 181 endpoints, including startup, count.
There is no lag fitting. Speed is a single sequential timing measurement per
case, not a statistical benchmark or a realtime/browser qualification.

The final regression run passed 34 tests: 16 embedded motor, 6 experiment,
5 reduced exploration and 7 fidelity tests. This includes an analytic motor
and circuit check for the separated derivative policies, capture rejection,
repeatability, and restoration of detailed finalist settings.
The native `finalist` command also produced `restored-baseline.spec.json`,
verified structurally identical to the original detailed recipe. It was not
simulated. `regression-tests.log` retains the test output.

Gait search remains paused. No hardware was operated. A future candidate must
still pass detailed-model checks; a passing baseline does not validate every
new gait or establish measured motor accuracy.

See [the reusable workflow guide](../../../primitives/reduced-exploration.md)
for prepare, qualify, journal initialization and detailed-finalist commands.
Qualification binds the tested baseline. Search elsewhere in a parameter space
still requires detailed checks and additional representative qualification.
