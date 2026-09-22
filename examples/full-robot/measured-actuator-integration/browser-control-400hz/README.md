# Bench-fit motors with 400 Hz control in the browser

Open **http://127.0.0.1:4182/?preset=robot-measured-400hz**, press Play, then hold
W/S for forward/reverse or A/D for gentle arcing turns. Releasing requests a
controlled stop; an airborne transfer finishes landing. Pause stops simulation
advancement. The viewer reports mean net travel over elapsed simulation time,
separately from the simulation/wall-time ratio. Save run retains the recipe,
seed and command events for shared-Rust replay.

This is live Rust/WASM simulation with all twelve detailed motor/gearbox models,
encoder quantization and the shared FPGA integer control law. It is currently
**too slow for real-time walking**: a three-second forward trial required 54.93 s
of worker round trips (about 0.055× real time) on the development Mac. It traveled
0.462 m with no detected fall, averaging 0.154 m/s net displacement over simulation
time. That short single-seed trial is not a sustainable or validated gait-speed
claim. Startup, rendering and host contention affect the separate wall-time result.
No desktop was foregrounded, audio played or physical motor commanded.

## Exact model and clocks

`prepare.mjs` derives this profile from the retained CAD-derived full-authority
contact-planned Rhai gait. It preserves authored mechanics and source CAD identity.
The three motor parameter sets are copied from the selected nominal ID 10, 11 and
12 full-drive tracking experiments, by hip/worm/foot role on each leg. This repeated
assignment is an explicit experimental assumption, not identification of twelve
physical units. `overrides.json` records source hashes and the previous overrides.

These are the existing **provisional fitted candidates**, not newly calibrated
motors. They failed the historical 0.264-degree closed-loop calibration criterion.
Loaded whole-robot accuracy is unqualified. Voltage is imposed at 11.1 V, temperature
is imposed, and feedback-to-application delay is an estimated 1.25 ms; there is no
claim that a measured physical twelve-motor bus provides this timing.

| Operation | Rate |
| --- | ---: |
| Motor sampling/integer control | 400 Hz |
| Native dense measurement profile | 200 Hz |
| Outer motion policy and browser action/observation | 50 Hz |
| Nominal detailed physics steps | 6,400 Hz |

Gains are Kp 4096, Kd 0, Kv 4096, PWM ceiling 1000, matching the firmware candidate.
The runtime retains motor/controller events between observation frames. Browser
live observations are 50 Hz; 200 Hz is the native capture profile, not a claim that
the UI exports 200 live frames per second. The FPGA logging implementation is
separate: `examples/actuators/hx30hm/hardware/2026-09-20-decimated-capture/`.

The explicit controller implementation identity was refreshed after the existing
power-of-two RTL gain support changed its source hash. `controller-identity.json`
retains both identities and the current source hash; preparation refuses a new
source change. The original rejected stale-identity readiness result is retained.
No implementation-identity validation was bypassed.

## Verification

- `verification/native-cadence.json`: eleven snapshots at 5 ms spacing through
  50 ms. Every motor's sampled/applied counters demonstrate 400 Hz control.
- `verify.mjs`: checks 228 resolved physical motor/driver parameters against the
  selected fit inputs, gains, latency, cadence, and all twelve bindings.
- `web/tests/measured-400hz.mjs`: headless rendered Chrome, actual worker commands
  and replies for W/A/D/S and release; no foreground computer use. Screenshots
  verify the model, controls and rate/speed labels. Actions are copied at send
  time and replies are matched by ID, including JSON transport.
- `web/tests/measured-400hz-performance.mjs`: deterministic common forward inputs
  in the same Rust/WASM worker. The retained three-second detailed run finishes
  without a simulation error or detected fall.
- Two timestep/probe-reuse candidates retain the same physical motor model and
  control clocks. Both are **rejected**, and their configs/results remain stored:

| Candidate step | Peak motor angle difference | Peak link-origin difference | Peak winding-current difference | Sim/wall |
| --- | ---: | ---: | ---: | ---: |
| 0.625 ms | 0.184° | 1.194 mm | 0.107 A | 0.046× |
| 1.25 ms | 0.246° | 3.238 mm | 0.205 A | 0.068× |

Comparison limits are 0.1 degree, 1 mm and 0.1 A, all required, over one second
against the 0.15625 ms detailed reference. Timings are exploratory and include
host contention. Neither candidate solves the real-time limitation, so the default
retains the detailed step. This does not establish absolute physical accuracy or
long-horizon numerical convergence. Terrain robustness, sustained stopping and
physical transfer remain unqualified.

`verification/summary.json` contains machine-readable results and source/artifact
hashes. The bundle manifest hashes WASM, inputs and browser source. The application
adds a reusable `--preset=ID` packaging option so this profile can be built alone.

## Reproduce

From the repository root, after building the current native Rust examples and a
release `sim-web` WASM artifact with the locked wasm-bindgen version:

```sh
node examples/full-robot/measured-actuator-integration/browser-control-400hz/prepare.mjs
WASM_ARTIFACT=runs/wasm-builds/scalar-release/target/wasm32-unknown-unknown/release/sim_web.wasm WASM_BINDGEN="$HOME/.cargo/bin/wasm-bindgen" node web/build-viewer.mjs examples/full-robot/measured-actuator-integration/browser-control-400hz/bundle --preset=robot-measured-400hz
node web/serve-viewer.mjs examples/full-robot/measured-actuator-integration/browser-control-400hz/bundle 4182
```

With that server running, in another terminal:

```sh
target/debug/examples/capture_embedded_window examples/full-robot/measured-actuator-integration/browser-control-400hz/scene.json examples/full-robot/measured-actuator-integration/browser-control-400hz/config.json 0 0.05 0.005 > examples/full-robot/measured-actuator-integration/browser-control-400hz/verification/native-cadence.json
node web/tests/measured-400hz.mjs
node web/tests/measured-400hz-performance.mjs
DURATION_S=3 PROFILES=reference OUTPUT_PREFIX=long- node web/tests/measured-400hz-performance.mjs
node examples/full-robot/measured-actuator-integration/browser-control-400hz/verify.mjs
```

Headless tests require Playwright and installed Chrome; override `CHROME_PATH` on
other hosts. The detailed trial cannot yet meet the project's real-time browser
requirement. The next performance work needs solver profiling and a separately
qualified browser fidelity profile; changing the logging rate cannot fix that
whole-robot simulation bottleneck.
