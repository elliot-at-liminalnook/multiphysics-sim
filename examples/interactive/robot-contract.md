# Authored robot inspection

`sim_domain_robot::contract::RobotDocument` retains original CAD JSON alongside
the parsed `PhysicalModel`. Its inspection distinguishes absent fields from the
runtime's legacy defaults. A missing mass, for example, appears as an absent
property and a defaulted JSON pointer even if the legacy loader resolves it to
1 kg. Presence in CAD does not establish measurement or calibration.

The versioned contract exposes category-qualified CAD identities, named
relationships (including loops and transmissions), selected physical properties
with units/frames, source evidence, material definitions, uncertainty and
identification. It lists fields ignored by `PhysicalModel`, so evidence such as
merged-member mass sources remains visible. Fixed CAD bodies merged into physical
links resolve through their original member names. Duplicate names remain warnings;
an ambiguous reference is an error. Missing CAD IDs use explicitly labelled name
fallbacks that are not stable under renaming.

All hosts use `sim_runtime::robot_contract::inspect`. It adds component parameters,
typed ports and validation declarations from the existing runtime registry. Native
CLI: `inspect_robot_contract model-or-scene.json`. WASM:
`inspect_robot_contract(document_json)`. The production worker accepts
`{type: "inspect_robot", document: originalModelOrScene}` without changing the
loaded simulation. `RobotDocument::matches_model` detects resolved-model changes
before attaching source claims; experimental overrides need their own record.

Four domain tests cover default awareness, typed prismatic/rotational properties,
identity preservation, aliases, ambiguity and topology. The runtime test uses
both the existing quadruped and the new wheeled CAD model. Browser comparisons
check the entire inspection result against native output, including registry
descriptions. The evidence manifest is `robot-contract-evidence-v1.json`.

This is an inspection API, not a complete dynamics contract or physical validity
certificate. Automatic episode binding, full observation/action channel schemas,
physical-value validation beyond parsing/reference checks, and policy morphology
tensors remain to be integrated. Original CAD input must be retained: inspecting
a reserialized legacy model cannot recover fields that were originally absent.

Episode actuator construction now consistently consumes `PhysicalRobot.model`,
where CAD identification has been applied once. Incremental motor parameters
previously used the unfitted scene input; see `identified-actuator-resolution.md`
for the reproduced mismatch and explicit-value/replay acceptance. This correction
does not complete original-input preservation or the resolved CAD contract.
