# Open connector authoring

`ConnectorKind` is now an owned semantic reference, like `QuantityKind`. Register
`ConnectorDefinition` and `QuantityDefinition` traits (or plain descriptors) with
a `BehaviorRegistry`, then use `ConnectorKind::named("package.connector", 1)` or
`ConnectorKind::from_descriptor(&descriptor)` in ordinary `acausal` declarations.
The independent `sim-example-diffusion` crate demonstrates the entire path.

Built-in associated constants remain available. Enum-style imports become
`use sim_core::connectors::{Electrical, Thermal};`. References are Clone, not Copy.
Compiler compatibility uses the complete namespaced identity and schema version;
matching dimensions or lane shapes do not connect distinct species implicitly.

Generic metadata consumers call `kind.resolve(registry.frozen_definitions()?)`.
The returned lane metadata refers to quantities in that same frozen catalogue.
The old no-argument lane/width helpers remain compatibility metadata for built-in
behavior implementations only. They are not an extension discovery interface.
Residuals use numeric lane slots; no registry lookup is introduced in residuals.

Component parameter declarations are supplemented with registered lane initial
values during `BehaviorRegistry::register`. Consumers should inspect registered
descriptors, not an unregistered `with_parameters` intermediate value. Runtime,
CAD/Rhai catalogue and diagram inspection share the same definitions.

Named composites declare ordered members and their local names in the registry.
Instantiation recursively creates child ports; `ModelWorld::connect` matches
semantic parent identities and expands matching composites into leaf nets.
Compiler validation rejects altered member order, ownership, names or schemas.
Nested pin locations and lane mappings resolve before stepping. Owned frame
members inside composite behavior slots remain explicitly unsupported.

Legacy strings and `{ "Composite": [ ... ] }` JSON still decode. Built-in strings
and anonymous composite JSON preserve their legacy wire representation. Composite
members deserialize into owned vectors and are dropped normally; there is no
`Box::leak`. A named reference serializes as `{ "name": "...", "version": 1 }`.
Anonymous composites have deterministic identities derived from ordered member
identities, preserving the earlier descriptor IDs. An old composite can also be
referenced by its resolved definition ID when its descriptor is registered.

This migration does not yet distinguish legacy dimensionless signal wildcards,
add committed through-flow sampling, or establish the <=5% stepping-performance
gate. Those remain requirements of the complete systems viewer plan.
