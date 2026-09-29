# Model annotation context

`build(document, registry, request)` prepares read-only engineering metadata for
annotation agents, inspectors and automation. Call it on a worker thread. It uses
`sim-system::flatten` and `sim-inspect::model::describe`; it does not compile or step
physics. This keeps inherited values and multi-terminal connection semantics shared
with the simulation path.

Requests accept a discussion ID, instance paths, or an empty model-wide scope.
Deleted targets are retained as missing. Packets include authored parameter bindings
and provenance alongside resolved numeric parameters, because the flattened model
alone does not retain all authoring metadata. Invalid models return authored context
and an explicit resolution error. Unknown target/discussion requests return errors.

The 128-instance focused expansion reports omissions and preserves complete net
terminal lists. Use a narrower target request to inspect omitted regions. Shapes and
placement are display-only. Observable definitions describe what could be measured;
they are not live samples or claims that the model has compiled successfully.
