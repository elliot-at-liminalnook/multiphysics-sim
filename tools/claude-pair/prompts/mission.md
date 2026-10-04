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
The shell is `sim-spatial`, and its target shape is
`docs/architecture/native-viewer.md`: read it, build toward it, and keep it
current. Don't create another competing viewer.

The user's native-first direction supersedes the old requirement to make the
browser the primary interaction surface. Preserve browser compatibility until
the corresponding native workflow is demonstrated. This is a user-facing
consolidation that now extends to CAD and hardware (user decision
2026-09-30): the browser's calibration and hardware pages move into the native
viewer, and CAD moves to Rust (architecture document §8–§9). Since 2026-10-03
everything runs in Rust, in the viewer's own process (AGENTS.md, and "Current
focus" below): RoboCAD's Python and the calibration and motor-bench servers
are references for behaviour, never started or called. Exact feature parity
with them is the bar, shown by tracing code paths. Explain any remaining
external UI requirement honestly.

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

## Current focus (the user, 2026-10-03): everything in Rust, in one process

The earlier focus (leg calibration, sim and leg side by side, the CAD editor
with annotations, the REST-built rover) is complete by reading. The user's
next requirement: **nothing the viewer does may rely on the RoboCAD server
or on a robot driver server.** Every workflow runs in Rust, in the viewer's
own process, on shared Rust libraries, as AGENTS.md now says. RoboCAD's
Python source stays readable as the reference for behaviour, but the viewer
never starts, calls or waits for it.

Until the user lifts this, choose, assign and do only work that removes a
server dependency, plus what it strictly needs. In order:

1. **The leg, driven in process.** The viewer's Leg calibration panel and
   gait playback (Sim, Leg, Both) talk to the leg through the shared Rust
   hardware layer directly, not over HTTP to `serve_actuator_calibration` or
   `serve_motor_bench`. Move their logic (about 3,000 lines that live only
   in `crates/sim-runtime/examples/serve_actuator_calibration.rs`, and the
   motor bench) into library modules the viewer calls through the jobs
   module. The examples become thin wrappers over that library, or are
   deleted once nothing needs them. **Every safety rule the server enforced
   moves with it unchanged**: one front end owns a motion session, STOP
   from every section, hold-to-move releases on focus loss and panel close,
   taught travel windows, watchdogs, recorded limits. The FPGA supervisor
   stays independent of all of it.
2. **CAD in Rust.** CAD mode opens, edits, saves and exports `.rcad`
   documents with no RoboCAD process: the document model and history,
   sketches and features, booleans, fillets and the rest of the modelling
   operations, topology naming and selection, mass and physical
   derivations, simrobot export, STEP/STL/3MF export, and annotations.
   Follow `docs/architecture/native-viewer.md` §9: derivations in Rust,
   then the geometry kernel from Rust. Choose the kernel (OCCT through Rust
   bindings, or a Rust B-rep kernel) on evidence, record the decision and
   its reasons, and keep the `.rcad` format compatible with existing files.
   Port one area at a time behind the same typed actions, and delete each
   `cad_client` path once its Rust replacement serves every caller.

Each epic names the server dependency it removes, and is done when that
workflow's code path, traced by reading, never reaches RoboCAD's REST
client (`sim_runtime::cad_client`) or a hardware server's HTTP client for
that workflow, and does what the reference did.

**Verify by reading, not by building or testing.** Don't compile, run the
viewer or run tests. Trace each workflow from the control a person uses to
its effect, compare it with the reference (RoboCAD's Python, the server
examples), and cite path:line. **Hardware stays a hard boundary: never
drive the real leg.** For steps that need it, leave the user a short run
sheet.

## Every screen is usable by an AI (the user, 2026-10-03)

Whatever you build in any mode, an AI must be able to drive it over REST, and
benefit from it, the same day. In the same change as the feature:

- typed REST commands that go through the same action, validation, undo and
  refusals as the UI (never a parallel handler), listed in `GET
  /v1/capabilities` with a working example;
- the mode's guide (`cad_guide` / `GET /v1/cad_guide` is the model: concepts,
  workflows in order, every command with an example, the rules) updated, or
  created for a mode that has none, so an agent starting cold begins at the
  right point;
- the reads an AI needs to work without seeing the screen (state, topology,
  renders, feeds), with a non-blocking way to wait (published resources,
  `/v1/events/…`);
- AI-facing features where the screen would benefit: the in-window assistant
  (CAD threads answer through `sim_agent`; reuse that pattern), agent-authored
  comments and links, named views or captures the AI makes for the person.

A feature without its REST surface and guide entry is not done.

## Freedom and hard boundaries

You work directly in the user's project folder and commit to its current
branch. Normal work is written and checked by reading: any command that builds
or runs code must finish within 10 seconds. There are no build or test passes:
verification is by reading. You have full control of this Mac's
tools: any shell command, any file in the project, package installs, network access,
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

## Decide, record, continue

This run is unattended, so nobody is waiting to answer questions. When a choice
would normally go to the user, make the best decision yourself and keep going.
That covers an ambiguous requirement, a stale fixture or hash, two reasonable
designs, or a gap just outside the current scope. Prefer the option that is
most reversible, keeps CAD and measured values as the source of truth, and
follows AGENTS.md.

Record every such decision in the `decisions` field of your response: the
decision, why, the alternatives you rejected, and what would make you revisit
it. The user reviews this log instead of being asked. Decisions that change
project behavior also go into the relevant repository document in the same
commit. A hard boundary is not a decision to make: route around it by choosing
other work, and record that. A blocked part of the work never stops the rest.

Completion means the agreed inventory is verified end to end in the native
viewer with recorded evidence, a clear launch path, and no hidden required
Python/browser UI detours. Compilation, test exit codes and an attractive
animation alone do not establish workflow parity. **No screenshots** (unless "This run" says screenshots are on). Don't run ui_capture, launch the viewer to look at it, or take screenshots, even if an assignment or an older document asks for them. The binary isn't rebuilt, so a screenshot would show stale code. Behavior, including what the UI shows, is established by reading the code and documentation.
Workflow parity is shown by tracing the code paths end to end. Never invent
evidence.
