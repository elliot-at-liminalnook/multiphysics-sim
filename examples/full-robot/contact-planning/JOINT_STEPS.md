# Independent force optimization for multiple steps per foot

The joint CAD planner now supports the [multi-step reference](CONTACT_SEQUENCES.md)
with a separate force curve for every stance. Force arrays use foot-major order:
each foot's original stance, followed by its configured additional stances.
`force` decisions still identify the physical foot's original stance;
`additional_force` identifies an additional stance by its zero-based index.
Both map into the same physical foot's CAD load Jacobian. Force endpoint zeros,
unilateral/friction constraints and nominal actuator-command limits are retained.

Contact timing metadata likewise distinguishes each stance's touchdown and
liftoff. Alignment includes every stance and all body/contact events. Each local
NLP still preserves a strict cyclic event ordering; this does not implement outer
contact-family selection or prove that alternative orderings are infeasible.

The exact wrench/command maps, full force derivatives, conic node groups, cached
geometry and native sparse Jacobian structure all use the per-stance mapping.
The existing single-stance force sampling path remains intact. The four older
alignment/basis/cache/support-space CLI audits explicitly reject multi-step
inputs; their single-stance behavior is preserved. Updated command-map and
force-Jacobian audits use the shared force accessors for either representation.

## Expansion and evidence

`JointContactMotion::repeated_cycle_with_variables` copies a motion, force curves
and bounded search into a longer common cycle. Each repeated step and body
control can then vary independently. Period/displacement boxes scale with the
cycle, phase/duration boxes transform into cycle fractions, and physical
force/position/angle bounds remain unchanged. The expansion CLI also duplicates
the planning mesh and rebuilds existing contact-relative timing. CAD, world,
actuators, physical gates and target speed remain identical.

The source is `joint-mesh-command-speed.recipe.json`. Its two-cycle expansion has
**884 variables: 152 motion and 732 force values**, 16 contact events, 16 body
controls and 532 evaluated frames. Both the expanded source and its conic force
solution pass the unchanged sampled planning gates at 0.025795773709373453 m/s.
This preserves the seed speed; it is not a faster walking result.

Verification completed:

- Twelve planner tests pass, including independent stance/foot loads, timing
  changes, invalid endpoints and corresponding bounded motion/force perturbations.
- All runtime examples check with native Ipopt and conic features enabled.
- The full 732-column command map and reordered 366-column subset agree with
  independent CAD evaluation within 1.137e-13 over 12,768 command rows. Cache
  baselines are byte-identical to uncached evaluation; duplicate variables reject.
- All 732 full force columns pass central-difference checks, with no fallback
  columns and maximum scaled error 1.135e-8 against the existing 1e-5 tolerance.
  These checks cover the expanded shifted-reference case, not every possible
  contact topology or motor-capacity switching point.
- The 732-variable conic problem returns `Solved` and passes the sampled physical
  gates, with zero force-box/cone violation. Independent balance and command map
  errors are 2.843e-13 and 1.137e-13. Replaying the original single-cycle conic
  input produces a byte-identical result to the archived earlier implementation.
- A four-model native check validates initialization and structure, retains a
  feasible seed, then deliberately exhausts its CAD evaluation allowance. Native
  status -13 is recorded with the explicit budget error; it is not convergence.

Complete terminal reports are in the gzip files listed by the [evidence manifest](joint-step-evidence.json).
Each archive was decompressed and compared byte-for-byte with its source. Code,
inputs, binaries and logs are hashed. An initial executable-wrapper build failed
on an inner documentation comment; the module wrapper was corrected and the
failed build log retained. Existing live executables were not replaced.

## Active speed search

`optimize_joint_ipopt_steps` runs the shared solver from the expanded conic seed,
with independent steps, forces, timing and body controls. Configuration retains
the existing 0.30 m/s experimental target, 100 native iterations and 20,000 CAD
attempts. These are bounds on this experiment, not physical speed limits or a
time limit on the goal. The launch record identifies its binary, inputs, process
and fresh snapshot directory.

The physical maximum remains unknown. Any improved sampled candidate still needs
dense checks, compiled-controller execution, timestep/sustained walking, WASD and
browser validation. Other contact orders and initial motion families remain open.
