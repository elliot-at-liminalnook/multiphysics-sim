# Systems builder

Build hierarchical multiphysics systems from the component library, drill into
any subsystem and implement it further, group parts into new subsystems, use
reference images, and run the result on the shared Rust runtime.

A **system file** (`*.system.json`, schema `sim.system/1`) is the source of
truth. The physical viewer, the schematic, their REST APIs and the `sim-system`
CLI all edit it through one command set with the same validation and one
shared undo history. A CAD file can link to it (path + SHA-256); CAD keeps
geometry, the system file keeps circuit and subsystem topology.

## Open the proving case

```sh
cargo build --release -p sim-spatial -p sim-viewer -p sim-runtime --bin sim-system
target/release/sim-spatial --system examples/systems-builder/motor-driver-board/board.system.json --schematic
```

`--schematic` opens the schematic in build mode on the same file. Edits in
either window appear in the other within half a second.

Second example, for learning the drivetrain library:
[`worm-drive/`](worm-drive/README.md). A motor, a worm gearbox and a winch,
with a lossless spur gearbox to swap in and compare.

## What you can do

| Action | Physical viewer | Schematic | CLI / REST |
|---|---|---|---|
| Place a library part | Palette → click (`/` searches) | Palette → click | `add_instance` |
| Select / multi-select | Click / shift-click a part | Instance checkboxes | `system_select` |
| Connect ports | Connect… → pick a port, then the other | Ports → click two | `connect` |
| Edit a parameter | Click it, type, Enter (`$name` inherits) | Type, Enter | `set_parameter` |
| Group selected parts | G | Group | `group` |
| Drill in / out | Open or Enter / Up or U | Open / Up | `system_level` |
| Swap implementation | Swap… (★ = same interface) | Swap… | `swap` |
| Give one placement its own copy | Make unique | Make unique | `make_unique` |
| Move | Arrow keys, PgUp/PgDn | – | `move_instance` |
| Reference image | Drop a PNG/JPEG; Calibrate by two points | Import by path | `sim-system reference` |
| Undo / redo | ⌘Z / ⇧⌘Z | Undo / Redo | `undo` / `redo` |
| Run | R (background thread, paced to real time at most) | Run → live panel | `sim-system run` |
| Learn about a part | Library → click: card with notes, equations, trade-offs, derived values, "pairs with" | – | `system_component` |
| Snap a fitting part onto a port | Inspector → Snap on → click (or card → Attach to …) | – | `system_suggest`, `system_snap` |
| Graph live quantities | Graphs (toolbar); Plot chips pin up to four | Live panel | `system_plot` |
| Save a subsystem for reuse | Save to library | Save to library | `sim-system library save` |

Editing inside a subsystem edits its **definition**, which every placement of
that definition shares; the status line says how many. Use *Make unique* to
change one placement only.

## The proving case: a motor-driver board

`motor-driver-board/board.system.json` is built by
`cargo run --release -p sim-runtime --example build_motor_driver_board`
through the same commands, and saves its subsystems to `library/systems/`.

- **Battery**: 3S pack (`robot.battery`).
- **5 V regulator** (`buck_5v`): 50 kHz PWM, PI loop on the sensed output,
  high-side MOSFET, Schottky catch diode, 47 µH / 100 µF, 10 Ω logic load.
  The switch heats a small copper node.
- **H-bridge** (`h_bridge_mosfet`): four 20 mΩ MOSFETs with body diodes,
  20 kHz sign-magnitude synchronous drive, shared heatsink.
  **Averaged alternative** (`h_bridge_averaged`) has the same ports; swap
  between them from the Swap… list.
- **Servo** (`hx30hm_servo`): HX-30HM motor with winding → case → air
  thermal path, driving an inertia with a viscous load.

Acceptance (`cargo test --release -p sim-runtime --test system_builder_proving`):

| Check | Tolerance |
|---|---|
| 5 V rail mean once settled | ±1 % |
| MOSFET heating vs I²·2R_on(T) | ±5 % |
| Averaged vs switching bridge, mean speed | ±1 % |
| 10 µs vs 5 µs step, rail and speed | ±0.5 % |

**Provenance.** HX-30HM constants are the provisional endpoint fit in
`examples/actuators/hx30hm/README.md` (from published ratings; not identified
from hardware). Board component values are illustrative design choices.
Diodes and body diodes are smooth piecewise-linear approximations, and
switching (capacitive) losses are not modelled. The switching board runs far
slower than real time; the averaged bridge is the real-time profile.

The board records `backward_euler` in its run settings: the implicit midpoint
rule leaves the endpoint values of algebraic quantities (such as the battery
terminal voltage) alternating after every switching edge.

See [display editing and discussions](DISPLAY-EDITING.md) for component icons, grid dragging, part/group comments and REST examples. **Placement is display-only; it does not change CAD geometry or simulated physical placement.**
