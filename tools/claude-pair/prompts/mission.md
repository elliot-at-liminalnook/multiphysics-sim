# One coherent Rust viewer

The user wants the existing physics-simulator capabilities consolidated into
one native Rust viewer, instead of switching between Python CAD windows,
browser interfaces, and separate Rust viewers. Improve coherence and finish
existing features before adding unrelated features.

Read AGENTS.md and applicable nested instructions, README.md,
systems-builder-progress.md, systems-viewer-plan.md, builder-roadmap.md,
web/README.md, and cad/README.md. Verify current code: progress documents are
leads, not proof. Inspect crates/sim-spatial, sim-viewer, sim-app, sim-inspect,
sim-diagram, sim-system, sim-runtime, sim-web, cad/robocad/ui, and web.

First inventory user workflows and their actual implementations: opening and
editing systems; CAD/geometry and physical properties; schematic and spatial
views; library/parameters/typed connections; live controls and graphs;
annotations and source links; robot teleoperation; recordings/replay;
experiments/gait studies; measured actuator models and calibration inspection.
For each record the current entry point, reusable Rust layer, native UI gap,
source owner, migration dependencies, and observable acceptance evidence.
Prioritize a small end-to-end native workflow. Choose and document the existing
native app that becomes the shell; don't create another competing viewer.

The user's native-first direction supersedes the old requirement to make the
browser the primary interaction surface. Preserve browser compatibility until
the corresponding native workflow is demonstrated. This is a user-facing
consolidation, not an instruction to rewrite every dependency: Python/OCCT may
remain behind a clean CAD service boundary while its workflow is accessible in
the native viewer. Explain any remaining external UI requirement honestly.

Architectural constraints:
- CAD remains the physical source of truth. Display layout is not geometry or
  a physics edit. Keep source-linked annotations, provenance and undo intact.
- Runtime, headless execution, replay and interactive simulation share Rust
  library types and one execution path. Never duplicate physics in the UI.
- Reuse registry metadata and shared validated commands. Put reusable behavior
  in shared crates; topology and parameters belong in models/examples.
- Keep expensive work off the UI thread, cancellable and observable. Preserve
  user edits, experiment records, rejected configurations and existing data.
- Keep fidelity labels, measured/derived/estimated distinctions, model identity,
  timestamps, schema versions and stale-frame handling truthful.
- No physical hardware operation, cloud provisioning, deployment, purchasing,
  remote pushes or publication. Inspect hardware workflows offline only.
- Work only in the assigned isolated checkout. Do not edit the source checkout,
  other worktrees, home settings, credentials or the coordinator installation.
  Exception: the worker may remove verified inactive, old, regenerable build
  output from other projects under the role prompt disk-space preflight policy.
  This grants no permission to edit other project sources or remove their data.
- Do not delete legacy implementations until parity and migration are proven.
  Do not weaken tests or fidelity gates to pass a migration.

Completion means the agreed inventory is verified end to end in the native
viewer with recorded evidence, a clear launch path, and no hidden required
Python/browser UI detours. Compilation, test exit codes and an attractive
animation alone do not establish workflow parity. If native GUI verification
is unavailable, mark it unverified and stop at that boundary; never invent it.

