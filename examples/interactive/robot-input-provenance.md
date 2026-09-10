# Robot input preservation

`Scene` retains the robot JSON received at the episode boundary before
`PhysicalModel` parsing supplies legacy defaults. Detailed sessions, incremental
sessions, environments, motion variants and experiment checkpoints use that same
Rust representation. An unchanged scene serializes the received robot document,
including omitted fields and metadata the physics parser does not model.

Editing `scene.robot` creates a versioned `robot_input` receipt on serialization.
Each override records its JSON pointer, current value and original value; a tagged
original distinguishes an absent field from an explicit `null`. Reordered entity
arrays preserve metadata by unique ID, falling back to a unique name. Array changes
and key removals use a containing-object/array receipt. The loader checks receipt
version, pointer syntax, overlap, current values and reconstruction of parsed edits.
These are consistency checks, not authenticated proof of CAD origin.

`Scene::input_binding()` exposes the original input inspection and overrides
separately. Sessions cache that binding; environment and incremental diagnostic
metadata expose it to all hosts. `robot_contract::inspect` includes the receipt
when inspecting an edited scene, so its original property claims cannot be mistaken
for an assertion that edited values were authored. Registry descriptions remain
shared with CAD and Rhai. Experiment identity includes the serialized input and
receipt; physics identity continues to describe parsed execution properties.

Use `Scene::replace_robot_input(document)` when explicitly accepting a new CAD
export. The replacement parses before changing the scene. For a caller-created
`Scene` struct literal, set `robot_input: None`; its serialization and metadata say
`parsed_model`, with original field presence unavailable. Loading that serialization
does not turn parser defaults into authored claims. A standalone legacy
`PhysicalModel::parse/load` does not retain the original document.

`PhysicalModel::to_json_value_checked` checks numeric representability before JSON
can silently turn nonfinite numbers into `null`, including floats inside options.
The two legacy unbounded gearbox limits serialize as omitted fields, restoring
positive infinity through their existing defaults. Their omission still appears
in the contract's default audit. Other nonfinite numbers are rejected by checked
serialization. This does not establish physical validity or calibration.

The focused fixture helper uses these APIs with any existing experiment:

```sh
cargo build --locked --release -p sim-runtime --example prepare_robot_input_experiment --example check_motion_experiment
target/release/examples/prepare_robot_input_experiment input-spec.json fresh-spec.json
target/release/examples/check_motion_experiment fresh-spec.json fresh-native.json
node web/tests/experiment.mjs isolated-viewer fresh-native.json fresh-browser.json fresh-browser-capture.json
```

The fixture explicitly omits ambient temperature, retains an unmodeled annotation,
then overrides the parsed temperature to 21°C. It is synthetic provenance evidence.
Tests also cover both CAD forms, absent versus null, entity reordering, invalid
receipts, nonfinite edits, explicit new inputs, and detailed/incremental replay.
The CI experiment workflow runs the fixture on the wheel robot and quadruped.

`robot-input-evidence-v1.json` durably retains four native cases, four isolated
browser captures, replay checkpoints, binaries, source hashes and test logs.
All four cases passed exact input-binding/inspection equality and same-host replay;
native/WASM trajectories met the explicit numeric portability budget. The runtime
checks cover 62 distinct tests (including repeated final checks), plus four domain
contract tests. CI is configured; these are local results, not a remote CI run.

Complete resolved-property validation, derivation/clamp provenance and promotion
of accepted overrides through CAD still need work. These short replay cases do
not qualify sustained locomotion, physical convergence, realtime performance or
learned morphology transfer.
