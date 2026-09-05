# CAD → motorized pendulum → controller → measured result

This is the reproducible acceptance example for the generic simulator. It authors
a mechanism through `robocad.Ops`, exports a v3 physical model, and runs that file
through `sim-cad` with a separate Python controller. It adds no mechanism-specific
solver or runtime.

The mechanism is a 16 × 12 × 120 mm PETG bar hinged at its top face, with a library
MG90S servo attached to a grounded bracket. The supervisor sends 0 rad initially,
+0.3 rad at 0.2 s, and −0.2 rad at 1.6 s. The servo's sampled feedback controller,
driver, electrical winding, compliant gearbox, inertia and thermal chain are
compiled as equation elements alongside the CAD-derived articulated body.

## Run

From the repository root, with Rust stable, Python 3.9–3.11 and a C/C++ toolchain:

```sh
python3 -m venv cad/.venv
cad/.venv/bin/python -m pip install -r cad/requirements.txt
cargo build --release --locked --bin sim-cad
cad/.venv/bin/python examples/motorized-pendulum/run.py
cad/.venv/bin/python -m pytest -q examples/motorized-pendulum/test_acceptance.py
```

On Windows, replace `cad/.venv/bin/python` with `cad\.venv\Scripts\python.exe`.
The runner selects `sim-cad.exe` automatically. CI currently enforces this
workflow on Ubuntu 22.04 / Python 3.11; other platforms can run it locally.
`--sim /path/to/sim-cad` and `--output /path/to/results` override the defaults.
For the checker tests with a custom output path, set `PENDULUM_RESULTS` to it.

Compilation and dependency installation are outside the measured budget. Exit
status 0 means all acceptance checks passed. A missing binary, crashed controller,
solver failure, timeout or failed numerical/performance check returns nonzero.
`acceptance.json` records failures, and per-run logs retain simulator diagnostics.

To run one experiment directly after export:

```sh
target/release/sim-cad run runs/motorized-pendulum/pendulum.simrobot.json \
  --seconds 3.2 --step 0.0005 --no-flex --no-contact \
  --controller examples/motorized-pendulum/controller.py \
  --python cad/.venv/bin/python \
  --controller-arg=--log \
  --controller-arg=runs/motorized-pendulum/manual.controller.jsonl \
  --out runs/motorized-pendulum/manual.simresult.json
```

`--controller` replaces the model's target supervisor through `control.external`.
The script uses the normal `simloop` protocol. Repeated `--controller-arg=...`
arguments are passed literally, without a shell. The model's servo firmware still
closes its position loop. Without `--controller`, `sim-cad` uses the model's
hold/trajectory settings as before. External controllers require a v3 model;
run each experiment separately instead of combining `--controller` with
`--montecarlo`.

To view the generated mechanism:

```sh
cargo run --release -p sim-app -- --scene cad --model runs/motorized-pendulum/pendulum.simrobot.json
```

The viewer starts with the CAD model's zero target; Up/Down moves the target.
The saved benchmark traces record the two-step experiment.

## Acceptance contract

[expectations.json](expectations.json) is the machine-readable source of budgets.
The nominal run is 3.2 simulated seconds, with a 0.5 ms backward-Euler step,
10 ms result samples, and 20 ms supervisor samples. A second identical run tests
repeatability, a 0.25 ms run tests timestep sensitivity, and a hold-zero controller
provides a falsifier for tracking. Each run starts from a newly compiled plant.

| Measurement | Requirement |
| --- | --- |
| CAD mass and each diagonal COM inertia | Relative error ≤ 10⁻⁶ against a uniform box |
| CAD COM and hinge position | Absolute error ≤ 10⁻⁹ m |
| Settled angle, over 1.2–1.5 s and 2.8–3.2 s | Every sample within 0.035 rad (2.01°) of the command |
| Mean shaft torque in each settled window | Within 0.002 N·m of `m g r sin(q)` |
| Peak absolute winding current | ≤ 0.8 A; tracking runs must exceed 0.01 A |
| Repeat angle/current/temperature/torque traces | Absolute difference ≤ 10⁻¹² on this machine/build |
| Half-step angle trace | Maximum difference ≤ 0.01 rad |
| Half-step winding current | RMS trace difference ≤ 0.05 A |
| Hold-zero falsifier | Every settled sample misses the requested reference by ≥ 0.15 rad |
| Solver step refinements | Zero, so retries cannot hide failure at the nominal step |
| CAD generation/export | ≤ 60 wall seconds |
| Each process run, including compilation of the model and controller startup | ≤ 60 wall seconds |
| Total export + four runs | ≤ 240 wall seconds |
| Individual simulation process timeout | 120 wall seconds |

The analytic bar mass is 0.0292608 kg, its centre is at z = 0.140 m, and the
hinge is at z = 0.200 m. The COM inertia about the hinge direction is
3.57371904 × 10⁻⁵ kg·m². These references are computed independently of the
exported mass/inertia values. Torque balance uses the measured angle.

The checker also verifies finite results, complete trace lengths, sample cadence,
controller sequences, actual commands and agreement between logged encoder
readings and plant results. Its regression tests corrupt real artifacts to prove
that wrong units, frozen motion, zero motor current, missing torque, repeat drift,
non-finite data, skipped commands and exceeded budgets fail acceptance.

## Artifacts and interpretation

`runs/motorized-pendulum/` contains:

- `pendulum.rcad`: editable source CAD assembly.
- `pendulum.simrobot.json`: the exported physical plant used by all four runs.
- `reference.json`: independent analytic geometry values.
- `{nominal,repeat,half_step,hold}.simresult.json`: sampled joint angle, motor
  current, winding temperature and shaft torque, plus aggregate diagnostics.
- Matching `.controller.jsonl` and `.log` files: supervisor frames and simulator output.
- `acceptance.json`: limits, observed values, pass/fail checks, timings, dependency versions, and model/controller/binary/source hashes.

The [simulation workflow](../../.github/workflows/simulation.yml) runs on pushes
and pull requests and uploads these artifacts even if acceptance fails. It also
checks all Rust targets and runs generic runtime/phenomena tests. The gallery's
separate host-dependent one-second viewer test is excluded from CI; this example's
explicit performance contract is enforced instead.

This verifies the implemented model and the CAD/control exchange. It does not
claim hardware-calibrated MG90S accuracy: motor parameters include library
estimates. The bearing is explicitly frictionless, the base is fixed, contact and
flexible-link dynamics are disabled, and no noise or uncertainty is injected.
Backlash, firmware quantisation, gearbox compliance and motor heating remain.
Timestep agreement bounds numerical sensitivity; it is not experimental validation.
Repeatability compares fresh runs of the same exported file on one machine, not
bit-identical results across operating systems. CAD document IDs may differ on a
fresh export; mass, inertia and the measured behavior must still meet this contract.
