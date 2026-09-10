# Identified actuator resolution

`PhysicalRobot::build_with` applies a model's identification once while assembling
the shared robot. `PhysicalRobot.model` is that resolved physical definition;
`Scene.robot` retains the original parsed input and active identification for
recording, reset and replay. Incremental motor, driver and firmware configurations
now read the same resolved model as the articulated mechanism and detailed plant.
Their numeric parameters remain visible in shared diagnostic metadata and use the
existing registry components and CAD parameter derivations.

Previously, the incremental motor bank read unfitted motor values from
`Scene.robot`, even though its articulated mechanism and the detailed motor plant
used the identified model. A fitted torque-constant scale therefore affected the
detailed plant but was omitted from the incremental motor bank. The regression
first failed on the actual motor parameter; it now checks explicit fitted values,
detailed and incremental physical trajectories, original-input retention and exact
replay without applying the fit twice.

The same resolution API can freeze an existing fit into an explicit experiment:

```sh
cargo build --locked --release -p sim-runtime --example resolve_identified_experiment
target/release/examples/resolve_identified_experiment identified-spec.json explicit-spec.json
```

The helper calls `PhysicalModel::apply_identification`, clears the active fit to
avoid reapplying it, and retains the parsed fit and input experiment reference in
`robot.source.resolved_identification`. It validates the resulting experiment
before creating a fresh output file. The receipt contains parsed identification
values, including legacy parser defaults; it does not establish which fields were
originally supplied or measured. Retain the input experiment as evidence. Accepted
hardware fits should be promoted through CAD's identification authoring, preserving
their source logs and uncertainty.

`resolved-identification-evidence-v1.json` retains synthetic fixtures on the
quadruped and wheeled CAD models: original, identified and explicit-value cases,
the previous and corrected native binaries, trajectories and replay checkpoints,
isolated browser captures and verification logs. The synthetic fixture scales one
motor's torque constant by 1.7 and the associated joint stiffness by 1.2. It is not
a hardware identification or a new optimized gait. The verifier compares the
physical trajectories and task outcomes of identified and explicit-value models,
reproduces the old mismatch, and checks that original and explicit-value baselines
retain their previous behavior.

The original quadruped speed fixture uses an explicit effective-servo profile;
its motor equations are bypassed, so changing an electrical torque constant does
not affect that profile. Those control cases are retained separately. The decisive
quadruped cases in `detailed/` use winding/rotor/gearbox integration with firmware
events, the same CAD robot, controller, commands, timestep and 100 ms horizon.
`detailed/profile.json` records the integration-profile changes, including removal
of retry options that belong exclusively to the mechanics-only adapter. The
verifier rejects effective-servo cases for this identification acceptance.

This fixes consistency of the physical definition consumed by the two execution
modes. The complete resolved CAD contract remains unfinished: original field
presence now survives Scene-based episode loading (see `robot-input-provenance.md`),
derivation floors and clamps need complete provenance, and full physical-property,
sensor and actuator validation still needs
integration. The short 30 ms wheel and 100 ms quadruped cases do not qualify
sustained locomotion, physical convergence, learned transfer or realtime performance.
