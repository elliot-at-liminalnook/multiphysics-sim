# Hardware measurements needed for this baseline

No hardware measurements have been supplied. `physical-audit.json` distinguishes
CAD declarations from measured quantities; agreement with those declarations
does not calibrate the robot. Preserve raw logs, setup descriptions, instrument
calibration, units, axis signs, voltage, load, temperature and timestamps. Identify
each motor/joint by its CAD name and record the CAD revision or artifact hash.

| Priority | Record on the hardware | Property to validate or promote into CAD |
|---|---|---|
| 1 | Output angle versus time at several measured loads and supply voltages, including unloaded motion; actual load torque and current where available | Motor `gearbox.max_output_speed` (rad/s), `gearbox.max_output_torque` (N m), and the shape of its loaded torque/speed relation. The fast model uses a simple effective servo approximation. |
| 2 | Timestamped requested and measured angle during steps and reversals, with load and supply conditions retained | Effective servo response, delay and damping; detailed firmware/electrical properties when identifiable. Structural joint stiffness and servo response are different quantities. |
| 3 | Tangential force and normal load while the actual foot material moves over the actual floor; normal force/displacement response | Material/world friction and contact compliance. This baseline reads `materials[material].friction["world"]`; changing `world.floor_friction` alone does not change its articulated contact friction. |
| 4 | Driver and driven angles, direction, reversal lost motion and input/output load through belt and worm assemblies | External transmission ratio, drive backlash and losses, with shaft/frame definitions explicit. Current external transmissions are ideal 1:1 belt and 5:1 worm relations. Do not fold unidentified transmission losses into an unrelated motor parameter. |
| 5 | Joint travel, interference/clearance observations, sensor inventory, timestamp latency, bias and noise | CAD limits and sensor definitions. Thirty-one moving joints lack limits and this quadruped declares no sensors; learned policy inputs remain ideal simulation observations. |

The local sensitivity screen makes loaded speed and available torque the first
priorities. Its perturbation ranges are exploratory or CAD uncertainty inputs,
not hardware confidence intervals. Reserve independent trials to validate fits.

The actuator inventory is in `physical-audit.json`. For example,
`-Y · Hip swing (belt) · HX-30HM` drives `-Y | Hip servo output`; the external
belt then drives `-Y | Hip swing`. Keep these coordinates distinct in logs.
Current values of 5.511566 rad/s and 2.941995 N m are declarations/ratings,
not measurements made by this project.

## Existing import and fit path

The compatibility `sim-cad fit` reader accepts `t` (seconds), `<joint>.angle`,
`<joint>.target` (radians) and optional `<motor>.current` (amperes). It ignores
voltage columns. Preserve voltage and torque in raw data even though this reader
cannot consume them. Its simple CSV reader lacks robust missing-data validation;
incomplete or nonfinite rows must not be treated as measured zeros.

This fitter adjusts joint Coulomb/viscous friction, drive backlash, structural
joint stiffness and selected motor torque constants. It does not identify the
fast profile's no-load speed, stall torque, effective servo stiffness/damping,
firmware delay, or floor material coefficients. An existing mismatch must also
be resolved before relying on its motor fit: optimization scales both torque and
back-EMF constants, while saved identification application scales only the torque
constant. These findings come from `read_log`, `apply_fit` and `fit` in
`crates/sim-runtime/src/physical.rs`, and `apply_identification` in
`crates/sim-domain-robot/src/model.rs`; no hardware fit was run or accepted.

Accepted measured properties belong in CAD with source logs and uncertainty.
The existing `ops.apply_identification(path)` / `POST /identification/apply`
route stores supported fit fields. Motor ratings, firmware, material properties
and sensors require their matching CAD properties. Use a separate revision and
preserve the baseline artifact.

After export, verify resolved physical values and regenerate the explicit
effective-servo profile where applicable. An identification block alone does not
update the fast recording's explicit effective-motor parameters. Rerun matched
experiments and retrain/revalidate predictors against the new physics context;
do not relabel old forecast models as calibrated.
