# Thermal residual units and acceptance evidence

The thermoelastic damping acceptance case was asking the nonlinear solver to
resolve heat-balance residuals below 1e-10 W while forming temperature differences
near 300 K. In the thinnest beam, layer conductance is about 91,909 W/K;
conductance times floating-point temperature spacing is already several
nanowatts. A first-step residual of 9.39e-9 W failed despite small corrections.

Compiled islands now allow explicit positive residual-row unit multipliers,
applied consistently to residuals and both supplied Jacobian parts after
elimination. Defaults remain unchanged. Full physical residuals remain available
through `residual_full`. Configure scales before integration or discard enclosing
solver caches. These scales change the units of numerical acceptance; they do
not change the roots or physical constitutive equations.

The thermoelastic example divides heat-balance rows by the layer conductance,
expressing their residuals as equivalent temperature errors. Mechanical rows,
material properties and physical validation assertions are unchanged. All five
beam thicknesses pass the first-step diagnostic, and all 11 original scenario
checks pass: damping versus theory, energy/entropy consistency, frequency,
peak-loss thickness, zero-coupling loss and rejection of negative conductance.
The energy/entropy identity error is 0.001537 against the existing 0.005 bound.

Reproduce with:

```sh
cargo test --locked --release -p sim-compile
cargo run --locked --release -p sim-phenomena --example thermoelastic_precision -- --unscaled
cargo run --locked --release -p sim-phenomena --example thermoelastic_precision -- --validate
```

The full phenomena acceptance run now gets beyond this case but fails separately
in Levitron Floquet analysis at t=2.7683241228718063 s, residual
1.1275702596898565e-10. That failure remains undiagnosed; the full suite is not
claimed green and no global tolerance was relaxed to suppress it.
