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
- Do not delete legacy implementations until parity and migration are proven.
  Do not weaken tests or fidelity gates to pass a migration.

## Freedom and hard boundaries

You work directly in the user's project folder and commit to its current
branch. You have full control of this Mac's tools: any shell command, any file
in the project, builds of any size, package installs, network access,
subagents, background processes, git operations in the workspace, and the
native viewer and its REST API. No command allowlist and no permission prompts.
Use judgment rather than asking.

A few boundaries remain because they protect people, hardware and the user's work:
- Never drive physical hardware (motors, servos, FPGA loads, serial writes to
  the leg). Hardware workflows can be inspected and simulated offline.
- Never start paid cloud compute, purchase anything, push to a remote, publish
  or deploy.
- The user may be editing this folder at the same time. Never revert, overwrite,
  stash or commit changes you did not make; stage only your own files or hunks.
  Never reset, rebase or amend commits that existed before the run.
- Never delete experiments, runs, recordings, calibration data, screenshots,
  receipts or other projects' sources. Regenerable build output is fair game
  when disk space runs short; record what you removed.
- Do not edit the coordinator's state, config, logs or receipts. They are the
  independent record that makes review meaningful.

Completion means the agreed inventory is verified end to end in the native
viewer with recorded evidence, a clear launch path, and no hidden required
Python/browser UI detours. Compilation, test exit codes and an attractive
animation alone do not establish workflow parity. Native evidence is available:
drive the real viewer with ui_capture.py (REST commands, `system_ui` control
activation, screenshots) and look at the images. Say what a capture proves and
what it does not (for example, a REST-activated control is not a drag gesture).
Never invent evidence.
