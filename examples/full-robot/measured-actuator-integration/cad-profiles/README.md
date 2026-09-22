# CAD actuator profile integration

The separate `robot-provisional.rcad` revision binds all twelve CAD motors to the
explicit HX-30HM family in `hx30hm-provisional-family.json`. Physical units are
unassigned and no rejected per-device fit has been promoted. The archive was
reloaded successfully; all 114 geometry entries match the preserved baseline
byte for byte. See `revision-receipt.json` for hashes.

## Implemented and verified

- Rust family/binding schema validates registry names, units, bounds, evidence
  references, versions and unambiguous CAD identities. Resolution is idempotent;
  per-unit deviations cannot accumulate on repeated builds.
- CAD commands and REST use that native validator, preserve failed edits, and
  support undo/redo. Profiles survive save/reload/physical export on the focused
  motor assembly. A legacy export without profiles still passes.
- Explicit PWM simulation consumes the resolved motor and driver values. A
  physical winding-current test demonstrates that profile resistance changes
  actual dynamics. Effective-servo and catalog-firmware bypasses are rejected.
- The registered `control.sampled_fixed_pd` component calls the exact integer
  law used in FPGA RTL. Unit tests cover reversals, quantization, exact command
  latency (including fractional sample periods), replay and encoder overflow.
  The shared motor event adapter can explicitly select this registered kind.
- All 16 shared motor adapter tests pass, including FPGA-controller-driven
  motion, command deadlines, state replay and analytic electrical/mechanical
  energy balance. The heat assertion now includes gearbox damping as well as
  winding resistance; numerical integration loss is accounted for separately.

Logs are retained beside this document. CAD profile acceptance is added to CI;
the CI job itself has not been run locally.

## Resolved full quadruped export failure

`failed-physical-export.log` retains the failed full export. The body
`27d71d0313bc` (`+X | Thigh cross-shaft`) is valid and closed according to the CAD
kernel, but its tiny solid (about 1.11e-6 mm³) collapses into coplanar triangles.
Its tessellation has zero volume and fails the watertight check. All other 113
bodies pass the solid-mesh audit. Finer tessellation and welding tolerances did
not fix it. The B-rep, source metadata and diagnostic logs are retained here.
No body has been removed. The exporter now records CAD-kernel membership for
valid closed solids whose tessellations are not watertight. Invalid CAD volumes
still fail. Surface distances retain their declared mesh/grid approximations.
The full 29-link, twelve-motor export now succeeds; its one CAD-membership
fallback is recorded in the collision derivation. Rust retains that evidence
through serialization. See [controller integration](../controller-integration/README.md).

## Remaining integration

Encoder origin/polarity, clock phase and initial reference bindings now select
the shared fixed-PD component in `EmbeddedSession` and the controller environment.
Connect the battery and power branches to the same incremental solve, retain
electrical comparisons in the viewer, and run the unchanged historical gait.
Bench calibration still fails the existing accuracy criterion; none of these
software tests establishes loaded motor or complete-robot accuracy.

## Reproduce

Build `sim-actuator-profiles`, then use the CAD virtual environment:

```sh
PYTHONPATH=cad cad/.venv/bin/python cad/scripts/prepare_measured_actuator_revision.py --resume
```

Omit `--resume` only when creating the revision for the first time. Add `--export`
to attempt the full geometry derivation; export work uses a content-checked cache
and records failures without discarding the verified archive. The script checks
the original CAD hash and profile evidence hashes before authoring or resuming.
