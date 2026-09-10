# Physics-bound trajectory forecasts

Version 2 trajectory models carry a compact `PhysicsContext`. Training-data
extraction, live controller prediction and embedded actuator-forecast construction
check it against the actual recorded or executing physics profile. A different
solver step, contact option, motor supply, physical definition or runtime source
cannot silently reuse the model. Prediction results return the checked context.

`RuntimeIdentity` records the library-source BLAKE3 and Cargo feature set. The build
script hashes workspace manifests/lockfile plus crate manifests, build scripts,
`src` resources and `native` sources, excluding `sim-app` and `sim-web` host
crates. Native and WASM builds from the same library sources and features share
this identity. Comments and other source-only edits conservatively change it.
It does not identify compiler flags, external system libraries, the executable,
hardware or numerical equivalence; retain the separate fidelity provenance and
binary artifacts for those comparisons. It is an identity, not authentication.

`PhysicsContext` includes versioned hashes of the complete parsed robot sections,
scene construction options and clock, and physics configuration. Full definitions
remain in the recording. The typed definitions still include legacy CAD defaults;
this is not a substitute for the strict authored/resolved robot contract.

The explicit exclusions separate episode state and control from parameters:

- Seed, episode horizon, starting coordinates and base pose do not bind a model.
- Controller programs, policy settings, motion gates and target trajectories are
  outside the physics hash. Controller-conditioned models check them through the
  existing `ControllerContext`; actuator-conditioned models consume explicit targets.
- Initial servo targets are command state. Supply voltage and winding temperature
  remain in the physics profile. Controller context still checks its initial targets.
- Rendering/report frequency, profiling, trial retention and contact-audit flags
  are diagnostics. Physics step, solver settings and subdivision/event options remain bound.

New configuration fields are included unless explicitly excluded. Canonical hashing
sorts object keys, preserves arrays and exact integer values, and normalizes signed
zero and integral floats within JavaScript's exact integer range. A changed section
produces a named-path error. Hashing the large CAD definition occurs once during
session construction; live queries compare cached compact profiles. Offline sample
extraction hashes the supplied recording once per extraction call, before
constructing sample windows.

These checks do not make the observations Markov-complete. Hidden actuator states,
controller memory and future disturbances are not fully represented in the current
features. A source/profile match also does not establish held-out prediction quality,
physical calibration, or transfer to another morphology.

## Versioning and replay

`ForecastRecipe.physics_context` is mandatory for controller requests. Bound models
use `TrajectoryForecaster.version = 2`; version 1 is retained only for legacy
unbound actuator models. A version/profile mismatch is rejected. New controller
recipes and initialized models use the bound path by default. Legacy models do not
gain a compatibility claim merely by continuing to deserialize.

New embedded recordings include `runtime_identity`. Legacy recordings retain an
explicitly unknown identity (`None`); source-bound training extraction rejects them.
Replay a legacy recording through the desired runtime to produce identified labels.
Replay can intentionally execute an old command schedule on new sources; its new
recording identifies the code that actually ran. A bound model embedded in a policy
must still pass the source/profile check before that runtime can be constructed.

`prepare_controller_forecast` creates an expected profile for its own runtime and
records both the source recording's identity and the expected identity in its scope
file. It does not relabel old measurements as new. `check_controller_forecast` then
requires identified captures matching that profile. The existing native/browser
workflow uses these shared tools on both CAD robot forms.

Fidelity `ExecutionContext` now also carries the optional recorded source identity,
so comparisons across known/unknown or changed library versions require an exact
declared context change. This remains separate from whether their errors are acceptable.

## Acceptance evidence

`physics-binding-evidence-v1.json` retains the final code inputs, builds, recordings,
models, raw browser forecasts and comparison reports. All 76 relevant tests pass.
The native and WASM source identities and canonical physics profiles agree on both
CAD robot forms. Profiles occupy about 2.4 kB each, independent of the large mesh
and controller definitions retained in the surrounding artifacts.

| Case | Recorded interval | Version 2 prediction queries |
|---|---:|---:|
| Wheeled velocity controller | 30 ms | 8 |
| Quadruped recorded controller | 100 ms | 3 |

Both hosts produce matching typed inputs, priors and predictions within the existing
portability budgets. Browser checks reject changed action units, physics hashes and
source identities while preserving the loaded environment. Native tests also reject
changed clocks/supplies before data extraction or actuator-forecast execution.
Equivalent bound/unbound actuator models preserve physical motion, and both real
robot captures preserve the prior checkpoint's parsed physical frame values exactly
after excluding wall-time measurements.

These short fitted examples use all samples for training. They do not establish
prediction accuracy on unseen trajectories, contact convergence, realtime inference,
or learned transfer across morphologies. The served user browser bundle is unchanged.
