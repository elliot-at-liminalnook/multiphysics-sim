# Reusable and parametric CAD components

## User decisions

- Shared component defaults, with explicit per-instance parameter overrides.
- Nested parametric components, with typed parent-to-child parameter mappings,
  branch-local overrides and cycle detection.
- One component definition can supply an entire articulated assembly, including
  a leg's bodies, joints, actuators, materials and declared physical properties.
- Each occurrence retains independent, stable body/joint identities and runtime
  state. Reusing a definition does not couple the four legs' commands or motion.

## Data and execution contract

1. A document embeds versioned component definitions. A definition owns source
   geometry once, a local frame, typed parameters, explicit generation/binding
   rules and named references to external bodies/joints. Library files transport
   the same definition and provenance between documents; opening a model must not
   depend on a mutable external file.
2. An occurrence stores a definition ID/revision, rigid placement, external
   bindings, parameter overrides and a stable source-node to occurrence-node map.
   Geometry in occurrences is derived, not independently saved as four B-reps.
3. The ordinary document nodes remain the execution/export surface. Materialized
   occurrence bodies and remapped joints feed the existing CAD-to-Rust exporter;
   there is no separate robot/leg physics runtime. Definitions themselves are not
   exported as extra physical bodies.
4. Parameter specifications and generation operations use one shared catalogue
   for validation, UI and REST. Expressions use a restricted arithmetic grammar,
   units, finite values and bounds. Imported solids are reusable immediately;
   editable design dimensions require explicit parameter bindings or construction
   rules. Do not infer missing design intent from their bounding boxes.
5. Prepare changes away from the UI thread, report progress and allow cancellation.
   Commit only a complete validated candidate against the captured document
   revision. Failure/cancellation leaves definitions and every occurrence intact.
   One edit is one undo/redo transaction. Previews never mutate source geometry.
6. Update shared defaults across all following occurrences; override only named
   parameters. Resetting an override resumes inheritance. A deliberate detach
   action converts an occurrence into ordinary editable CAD nodes.
7. Geometry regeneration must carry explicit physical metadata and frame changes.
   Missing external references, unsupported metadata transformations, cycles and
   incompatible parameters are errors, not defaults to world/zero/empty values.

## Acceptance cases

- A generic articulated component placed four times stores source geometry once,
  resolves four independent part/joint ID sets and preserves relative geometry.
- One shared parameter edit updates all following instances; one override affects
  only its occurrence, survives default changes and can be reset.
- Rotation/translation transform joint frames and motor/sensor metadata correctly.
  Internal and external references remain valid after update, undo/redo and reload.
- Invalid values, incompatible units, broken references and cancelled preparation
  make no partial changes. Unsaved work is retained.
- Save/load and library export/import retain IDs, units, provenance and definitions;
  rendering and existing physics export consume the same materialized result.
- Use a separate quadruped candidate for canonical-leg reuse. Check all four legs
  against the current baseline before replacing any original geometry; retain and
  report differences rather than forcing nominally repeated parts to be identical.

## Full quadruped migration

The full model uses a shared leg family with four named as-modeled variants.
Variants retain distinct topologies and stable source IDs; shared typed parameter
bindings reach their nested mechanical components. The chassis and complete robot
are components too. The existing drive ratios/home angles and coherent whole-model
placement are explicit recipes. Imported shape histories are retained as source
B-reps, not inferred. The conversion is backup-gated and checks byte equality of
all geometry assets, original physical metadata, joint records, rigid-link grouping,
parameter inheritance/overrides, undo, and save/reload.
