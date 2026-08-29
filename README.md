# Multiphysics Sim

An executable Rust vertical slice of a pluggable multiphysics simulator: controller → H-bridge → DC motor → 10:1 gearbox → lead screw → linear carriage, with a live Bevy 3D view.

## Run

```sh
cargo run -p sim-app
cargo run -p sim-test -- all
```

Left-drag to orbit; scroll to zoom. Use `↑`/`↓` to move the target, `O` for an obstruction, `B` for brownout, `Space` to pause, and `R` to reset.

See the [architecture](index.html) and [as-built implementation plan](actuator-slice-plan.html).
