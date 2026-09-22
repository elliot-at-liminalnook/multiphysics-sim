# External diffusion example

This crate defines a concentration quantity and species connector through the
public traits, then registers storage, diffusion and boundary components in a
caller-supplied `BehaviorRegistry`. It is not added to the runtime's built-in
physics catalogue. The standalone export binary supplies the registration and
passes an ordinary `ModelWorld` to the generic inspector. The existing viewer
loads that description without a diffusion dependency or domain-specific code.

## Equations and units

A storage volume has concentration `c` in mol/m³ and volume `V` in m³. Its amount
is `n = V c` in mol and its inward flow is `V dc/dt` in mol/s. A diffusion path
between nodes a and b contributes `G (c_a - c_b)` at a and its negative at b,
where `G` has units m³/s. Node contributions sum to zero. Thus a volume joined to
a fixed reservoir obeys `V dc/dt = -G (c - c_boundary)`. The boundary's multiplier
is the molar flow it absorbs; negative values mean it supplies material.

These are illustrative, well-mixed, linear transport models. They omit advection,
reaction, spatial geometry, species-dependent chemical potentials and calibration.
The connector explicitly declares energy diagnostics unavailable: concentration
multiplied by molar flow is not power. No energy or entropy acceptance is claimed.

The exported example uses `V = 2 m³`, `G = 0.5 m³/s`, `c(0) = 3 mol/m³` and
`c_boundary = 1 mol/m³`. Its exact solution is `1 + 2 exp(-t/4)` mol/m³.
For a closed pair with volumes 2 and 5 m³, initial concentrations 3 and 0 mol/m³,
and conductance 0.5 m³/s, total amount is 6 mol and the concentration difference
is `3 exp(-0.35 t)` mol/m³.

## Acceptance

```sh
cargo test --locked -p sim-example-diffusion -- --nocapture
cargo run --locked -p sim-example-diffusion -- \
  examples/systems-viewer/diffusion.description.json
cargo run --locked -p sim-viewer -- \
  --description examples/systems-viewer/diffusion.description.json
```

Implicit midpoint acceptance uses predeclared bounds: relaxation error at 4 s
below 2e-4 mol/m³ for h=0.2 s; halving h gives an error ratio between 0.23 and
0.27; h=0.01 s gives error below 4e-7 mol/m³. The closed pair is checked every
0.1 s through 10 s with h=0.01 s: amount error below 1e-9 mol, difference error
below 2e-6 mol/m³, and nonnegative concentrations. No solver tolerances change.

Further tests check model roundtrip, semantic connector mismatch rejection,
registry-generated parameter units and Rhai catalogue lanes, generic inspection
and routing, and a named nested socket with two distinct diffusion channels.
The socket test checks recursive expansion, connection, pin mapping, integration,
inspection parent chains and rejection of a tampered serialized member map.

The workspace CI test command includes this crate. Native and WASM build checks
are separate from numerical validation; this crate makes no sim-to-real claim.
