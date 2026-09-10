# Fast WASD gait — bounded development session

The selected gait sustained **62.5–63.1 mm/s** over a 60-second forward, turning,
reverse and stop sequence. Maximum loaded-foot material motion was **3.54%** of
total body travel; maximum tilt was **0.0100 rad**. This is about 2.5 times the
previous 25–27 mm/s paired gait. It is a simulation result using provisional
actuator properties and ideal joint observations, not calibrated hardware speed.

Open the local review at **http://127.0.0.1:53646/?preset=fast-wasd-paired**.
Reset, press Play, hold W/S to translate and combine with A/D to steer. Release
finishes the transfer and brakes. A/D alone does not pivot. The review preset is
20 seconds; `sustained.*` records the separate 60-second test.

## What changed

- A 52 mm stride, 0.8 s cycle, 0.32 s swing and 8 mm lift use a longer horizontal
  swing while raising early and lowering late. Quintic foot paths retain zero
  endpoint velocity. Nominal peak world swing velocity is `1.875 L / T_s`.
- The effective servo law is `tau = K(q_command-q) - D*qdot`. Feedforward
  `(D/K)*qdot_reference`, with `D/K = 0.020 s`, reduces nominal motion lag without
  changing the modeled actuator authority. Saturation remains active.
- Velocity acceleration is bounded at 0.4 m/s². Stops and reversals brake in an
  all-stance interval at up to 0.8 m/s² and settle for 80 ms before resuming.
  Phase advancement cannot start another lift while braking.
- `control.command_lease` is a reusable Rust component, with registry parameters,
  typed ports and a Rhai binding. Duplicate/out-of-order packet sequences expire
  after 250 ms, sampled at 20 ms. The hardware host must keep the controller
  ticking independently of the transport and forward actual packet sequences.
- A bounded, CAD-derived 0.5 mm local distance grid corrects four false coarse-grid
  overlaps. The export took 5.1 s rather than refining an entire link for minutes.
  The parent grid and authored joint exclusions remain. The transition is an
  approximate blended field; this is not exact CAD collision certification.

This follows the physics-reference idea discussed with
[Walk the PLANC](https://arxiv.org/html/2601.06286v1): derive useful motion from
mechanics, then evaluate the actual controlled system. This implementation is
a Rust/Rhai controller, not the paper's humanoid model or a trained neural policy.

## Measured evidence

| Test | Result |
|---|---|
| Detailed 20 s WASD sequence, 1.25 ms step | 62.8 forward / 62.5 reverse mm/s; 4.77% slip |
| Fine 0.625 ms check | 0.66 mm maximum body difference; 0.12% speed difference; 4.93% slip |
| Selected 5 ms profile, 20 s | 2.15 mm maximum body difference; 0.34% speed difference; 4.36% slip |
| Selected profile, 60 s | 62.5–63.1 mm/s; 3.54% slip; 3.47 m total body path |
| Release during 20 s sequence | Below 1 mm/s within 0.44 s; 17.4 mm additional travel |
| Frozen nonzero command packet | Below 1 mm/s 0.42 s after loss; 18.6 mm additional travel; fresh reversal resumes |
| Native/WASM parity | Passed; maximum numeric difference 9.34e-10; exact replay and reset |
| Rendered browser | Approximately 0.67× realtime, 71 ms p95; **fails 1× / 20 ms targets** |

Slip integrates force-weighted foot material velocity while neighboring recorded
samples both carry at least 1 N. Its denominator is total body path in reversing
runs, and net advance in short straight trials. Those ratios are not directly
interchangeable. Stopping depends on the phase at release: the 60 s run stopped
in 0.20 s / 6.7 mm. The frozen-packet test advances physics during the outage;
pausing a browser is not a physical hardware outage test.

`geometry-status.json` and each `*.geometry.json` preserve full sampled inter-link
audits and lift reports. Checks use saved Rust poses, CAD-derived distance fields,
and retained authored exclusions. Inter-link impact forces remain omitted in this
profile, so these independent checks matter. They do not cover every point or
every instant between recorded frames. The 24 fixed lift windows cover the first
forward section; they do not qualify every later reversal or every sustained lift.
The dropout report uses the 14 windows before expiry; the original nominal-window
report is retained separately, including ten missing lifts after the commanded
stop. Those cancelled lifts are expected behavior, not continued walking.

## Exploration and remaining limits

`speed-summary.json`, `human-summary.json`, `profile-summary.json` and
`long-summary.json` retain results. Faster-cadence cases reached 110 mm/s but
slipped excessively. Longer strides at 75–100 mm/s, centered strokes, and 30°/45°
hip postures were also tested; they did not improve the accepted result. Other
paths failed collision screening and remain recorded. No global optimum is claimed.

Browser performance is the main unmet requirement. Larger integration steps
exceeded the 3 mm trajectory-error gate; lower solver reuse did not help, and a
tighter adaptive Newton budget failed at 6.06 s. Reusing viewer arrows and readout
elements improved measured wall pace slightly, without changing simulation.

CAD revision 1357 and SHA256
`2fc4523f1fefa5ff3530f1d6814ec4f1a9e5c345b4ce8a924686cd654e3c0589`
remain authoritative. No mass, transmission ratio, torque/speed limit or source
geometry was increased. `local-refinement.json` records the export derivation;
accepted export settings can be carried into the CAD export through the shared
`signed_distance_grid` bounded-query API. Null CAD limits remain unknown hardware
limits. Real encoder/IMU bindings, firmware timing, actuator calibration, terrain
robustness, emergency disable and deployment remain unfinished.

## Reproduce

Use the versioned code/CAD and exact JSON recipes. `evidence-index.json` records
large-file hashes; concatenate `evidence.part-*` to an archive and extract into
this directory to restore them. `browser-bundle.tar.gz` contains the reviewed
standalone browser assets, including the matching WASM and input recipe.

```sh
cat examples/full-robot/fast-wasd/evidence.part-* > /tmp/fast-wasd-evidence.tar.gz
tar -xzf /tmp/fast-wasd-evidence.tar.gz -C examples/full-robot/fast-wasd
mkdir -p runs/fast-wasd/review
tar -xzf examples/full-robot/fast-wasd/browser-bundle.tar.gz -C runs/fast-wasd/review
node web/serve-viewer.mjs runs/fast-wasd/review 4173
```

The generation and analysis scripts are retained beside the inputs. Trial runners
refuse to overwrite captures; use a new output directory for a new experiment.
`PLAN.md` and `CHECKPOINT.md` preserve the 30-minute checkpoint and one-hour review
limit. Original-root CAD/user edits and earlier viewer sessions were preserved.
