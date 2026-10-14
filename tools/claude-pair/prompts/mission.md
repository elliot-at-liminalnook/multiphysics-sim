# Design to delivery: the robot lifecycle in one Rust viewer

The user wants the native Rust viewer to carry a robot through its whole life:
design it, simulate and test it, source its parts, make the parts, assemble
it, bring it up, ship it, and keep track of every unit and revision after.
Today the viewer is strong at design, simulation and test, and stops at
"Make" (part files exported from the tested CAD revision). This run builds
everything after that point, as features a person and an AI can use, and
refines the user interface along the way so it is pleasant to use.

Read AGENTS.md and applicable nested instructions, README.md,
`docs/robot-pipeline.md` (the Design › Model › Test › Learn › Make strip
this run extends) and `docs/architecture/native-viewer.md` (the shell's
target shape: build in it, keep it current, don't create a competing
viewer). Verify current code: progress documents are leads, not proof.

## Current focus (the user, 2026-10-13): every robot has an algorithmic path to being built

Every robot, assembly and printed part must have a computed, repeatable path
from a CAD revision to a finished, shipped unit. Same inputs, same plan.
Nothing on that path is a hand-kept list that drifts from CAD.

The lifecycle, and what each stage needs:

1. **Parts and sourcing.** One parts catalog: every purchased component has
   a record (manufacturer, part number, the suppliers that sell it with
   their SKUs, links, price breaks, minimum order, lead time, country of
   origin, alternates). Each value is dated and labelled with its source
   (quoted, observed on a supplier page, measured, estimated) the way
   actuator values are. CAD components reference catalog entries; the
   catalog never duplicates what CAD owns (geometry, mass, material).
2. **Bill of materials from CAD.** Computed from a `.rcad` revision:
   purchased parts, printed and fabricated parts, fasteners, raw stock,
   per assembly and rolled up for N units, with cost, mass and longest lead
   time. Missing data stays visible as missing; it is never filled in.
3. **Fabrication.** Printed parts become print jobs: plates, settings from
   the print registry, printer time and material use, with the part strength
   checks that already exist. Other processes (laser cut, CNC, sheet) enter
   as process types with registry values, not special cases.
4. **Assembly.** An assembly sequence derived from the CAD hierarchy,
   joints and fasteners: precedence, tools, fastener counts and torques,
   step-by-step views (exploded and highlighted in the viewer), and checks
   that the sequence is buildable.
5. **Build orders and inventory.** Plan a build of N robots: what is on
   hand, what to buy (grouped by supplier, respecting minimum orders and
   price breaks), what to print and when, and what blocks the build. Track
   stock and where it lives.
6. **Bring-up and quality.** Per-unit test and calibration steps that link
   the existing acceptance tests and hardware front end, with a receipt per
   unit.
7. **Shipping.** Packaging, parcel size and weight from CAD, contents
   lists, customs data (HS code, country of origin, value), and a shipping
   manifest per order.
8. **Traceability and change.** Each unit records its CAD revision, part
   lots, print jobs, calibration receipts and serial number. A CAD change
   shows its impact: which parts, stock, open builds and shipped units it
   affects (where-used, change orders).

Existing pieces to build on, not beside:
- the robot project (`sim_runtime::robot_project`, `crates/sim-spatial/src/project/`)
  and its step strip; Make becomes the first of the new steps;
- printable parts: `sim-print` (registry, plan, plates, voxel stress,
  joints, promotion) and CAD print (`crates/sim-spatial/src/cad/print/`,
  including the fastener tool);
- the actuator registry (`sim_runtime::actuator_registry`), the model for
  one accepted source with provenance and promotion;
- `sim_cad::physical` export (mass, materials, motors from the library) and
  the component registry, so the BOM reads what CAD already knows;
- datasheets (`crates/sim-runtime/tests/datasheets.rs`) and candidate and
  mass budgets such as
  `examples/full-robot/measured-actuator-integration/battery-selection/`.

Choose, assign and do work that moves a real robot further along this path,
in roughly the order above. Each epic names the lifecycle workflow it makes
possible (for example "plan a build of 5 rover kits: BOM, purchase list by
supplier, print plates"), and is done when that workflow runs from the
control a person uses to its result, traced by reading.

The earlier focus, everything in Rust in one process, still holds as a rule
(AGENTS.md): no new call to RoboCAD's REST client or a hardware server, and
a lifecycle workflow that needs a CAD operation still on that client ports
it first. Finish the in-flight CAD batch; after it, choose server-removal
work when a lifecycle workflow depends on it.

## Think big: a professional tool, one coherent journey (the user, 2026-10-13)

The bar is a professional product that a person trusts from the blank
window to the shipped robot: comprehensive, internally consistent, and
obviously one project rather than a collection of features. Propose and build
large, ambitious epics, including features nobody asked for yet that a
serious robot company would expect or envy. Small, safe increments are the
exception, not the default.

The kind of ambition that fits:
- **Release a revision.** One action freezes a tested CAD revision into a
  manufacturing package, the way professional PLM tools do: BOM, sourcing
  snapshot, print plates and settings, an illustrated assembly book, the
  test and calibration plan, packaging and shipping spec, all stamped with
  the revisions they came from and diffable against the last release.
- **A build room.** Plan and run a production batch: printer and bench
  schedules, kits per unit, live progress per serial, blockers and the
  critical path, with the AI drafting the plan for the person to approve.
- **Assembly you can watch.** Each assembly step animated in the viewer from
  the CAD hierarchy (parts flying in along their insertion direction,
  fasteners highlighted, tools named), playable forward and back, exported
  as a printed book.
- **Design for manufacture and cost.** Checks that flag parts that can't be
  printed, fastened or reached; cost and lead-time rollups that update as
  CAD changes; cost-down and second-source studies that propose changes and
  re-run the robot's acceptance tests before anything is accepted.
- **Supply risk.** Single-source parts, long lead times, parts near end of
  life, alternates with their fit and simulation impact checked.
- **A life record per robot.** Every unit's timeline from print to field:
  revision, parts lots, calibration receipts, test results, repairs and
  returns, with field data flowing back into design (a digital twin that
  starts from the unit's own measured values).

These are examples, not a list to work through. Each must still be real:
built on the shared libraries, honest about estimates, and finished as a
workflow rather than a demo.

**Internal consistency is a feature.** Every lifecycle stage uses the same
project, the same revision identity, the same catalog, the same selection
and annotation model, the same layouts and controls, and the same words for
the same things. A person always knows which robot and revision they are
looking at, where it is in its life, what the next step is and what blocks
it. A feature that introduces its own vocabulary, its own file type for
something the project already records, or its own way of navigating counts
against itself.

## Rust and modern Bevy are the medium

Every lifecycle feature is Rust: data and algorithms in shared library
crates, usable headless and over REST; screens in `sim-spatial` on Bevy
0.19.1. Use what the engine gives: states for modes and screens, feature
plugins and public system sets, messages and observers, picking, the
`bevy_ui` widgets under the UI kit, `ViewportNode` for embedded 3D views
(assembly steps, part previews), the asset system for meshes and images.
`bevy.md` says how. A feature that hand-rolls what Bevy provides is not
done.

## Make it polished, and make the flow obvious

The interface is part of the product, not a wrapper around it. A person new
to the tool should understand the flow without reading a manual: where they
are, what this screen is for, what to do next, and how this screen connects
to the stages before and after it. The project's step strip (Design ›
Model › Test › … › Ship) is the spine; every lifecycle screen hangs off it
and returns to it.

Refine the interface while you build, and give it whole epics of its own
when the flow is confusing. Every epic leaves the screens it touches better
than it found them:
- professional editor layouts built from the UI kit, not ad-hoc panels; one
  visual language across every mode (spacing, type scale, colour, icons,
  states);
- clear empty states that say what to do next, and steps that are not
  ready say why;
- units on every number, rounded and steady (labels don't jitter), tables
  that sort and filter, and nothing important hidden behind a hover;
- long work (BOM rollups, plate planning, supplier imports) off the UI
  thread with progress and cancel; no stalls;
- keyboard paths for frequent actions, and undo for every edit.

Polish that only reshuffles is not refinement. Name the friction a change
removes.

## Architectural constraints

- CAD remains the physical source of truth. The catalog owns sourcing data;
  the actuator and print registries own their measured values. Nothing is
  hand-copied between them. Display layout is not geometry or a physics
  edit. Keep source-linked annotations, provenance and undo intact.
- Plans are derived, deterministic and versioned: a BOM, print plan,
  assembly sequence or build order records the CAD revision, catalog
  revision and settings it came from, and goes stale visibly when they
  change.
- Runtime, headless execution, replay and interactive simulation share Rust
  library types and one execution path. Never duplicate physics in the UI.
- Reuse registry metadata and shared validated commands. Put reusable
  behavior in shared crates; robot-specific parts lists, suppliers and
  parameters belong in data files and examples.
- Keep expensive work off the UI thread, cancellable and observable.
  Preserve user edits, experiment records, inventory, build and shipping
  records and existing data.
- Keep fidelity labels, measured/derived/estimated/quoted distinctions,
  dates, model identity, schema versions and stale-data handling truthful.
  A price or lead time without a date and source is not data.
- Human- and LLM-editable files (catalog entries, build orders) convert
  exactly to shared library types and fail with errors that name the path.
- Do not weaken tests, fidelity or safety gates to land a feature.

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
- the reads an AI needs to work without seeing the screen (state, BOMs,
  plans, renders, feeds), with a non-blocking way to wait (published
  resources, `/v1/events/…`);
- AI-facing features where the screen would benefit: the in-window assistant
  (CAD threads answer through `sim_agent`; reuse that pattern), agent-authored
  comments and links, named views or captures the AI makes for the person.
  An AI that can read a BOM and propose alternates, or draft a build plan for
  review, is the kind of feature that belongs here.

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

A few boundaries remain because they protect people, hardware, money and the
user's work:
- Never drive physical hardware (motors, servos, FPGA loads, serial writes to
  the leg). Hardware workflows can be inspected and simulated offline.
- Never purchase anything, place or submit an order, request a quote,
  create an account, use credentials or contact a supplier or carrier.
  Supplier and carrier integrations import files or read public pages; any
  that would need an API key take it from configuration the user provides
  and never commit it. Record the URL and date of anything read from the web.
- Never start paid cloud compute, push to a remote, publish or deploy.
- The user may be editing this folder at the same time. Never revert, overwrite,
  stash or commit changes you did not make; stage only your own files or hunks.
  Never reset, rebase or amend commits that existed before the run.
- Never delete experiments, runs, recordings, calibration data, inventory,
  build or shipping records, screenshots, receipts or other projects'
  sources. Regenerable build output is fair game when disk space runs short;
  record what you removed.
- Do not edit the coordinator's state, config, logs or receipts. They are the
  independent record that makes review meaningful.

## Decide, record, continue

This run is unattended, so nobody is waiting to answer questions. When a choice
would normally go to the user, make the best decision yourself and keep going.
That covers an ambiguous requirement, a data format, two reasonable designs,
or a gap just outside the current scope. Prefer the option that is most
reversible, keeps CAD, the registries and the catalog as the sources of
truth, and follows AGENTS.md.

Record every such decision in the `decisions` field of your response: the
decision, why, the alternatives you rejected, and what would make you revisit
it. The user reviews this log instead of being asked. Decisions that change
project behavior also go into the relevant repository document in the same
commit. A hard boundary is not a decision to make: route around it by choosing
other work, and record that. A blocked part of the work never stops the rest.

Completion of an epic means its lifecycle workflow runs end to end in the
native viewer and over REST, traced by reading, with a clear launch path and
honest labels on every estimated or unverified value. Compilation, test exit
codes and an attractive screen alone do not establish a working workflow.
**No screenshots** (unless "This run" says screenshots are on). Don't run ui_capture, launch the viewer to look at it, or take screenshots, even if an assignment or an older document asks for them. The binary isn't rebuilt, so a screenshot would show stale code. Behavior, including what the UI shows, is established by reading the code and documentation.
Workflows are shown by tracing the code paths end to end. Never invent
evidence, prices, lead times or supplier data.
