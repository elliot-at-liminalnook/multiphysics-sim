# Robot kit

Five starter robots built from the component library, each with a saved
study that shows one real engineering trade-off. Every part in them is
annotated: open it in the builder's library for how it works, its equations,
trade-offs, limits and datasheet. `library/CATALOG.md` lists all 96 parts.

```sh
cargo build --release -p sim-spatial -p sim-runtime --bin sim-system
target/release/sim-spatial --system examples/robot-kit/rover.system.json
```

Press **R** to run, **Graphs** for live plots, the **Studies** tab to rerun
the saved study, or `sim-system study FILE NAME` for the table.

| Robot | What it shows | Saved study |
|---|---|---|
| `rover` | A 2 kg two-wheel rover: 3S LiPo, two H-bridges, two 37 mm 30:1 gearmotors, tyres with grip limits. Cruises at 1.11 m/s at 80 % throttle, brakes when the throttle drops. | `hills`: each 10° of slope costs speed and multiplies current |
| `drone` | A 1.2 kg quadcopter holding altitude: four 2212 motors with 10″ props (≈ 4.4 A each at hover), altitude PID with hover feed-forward, the motors' windings warming. | `prop_size`: bigger props hover on less battery current |
| `sea_arm` | A series-elastic joint (gearmotor → spring → link) lifting a 0.5 kg link to 1.2 rad against gravity under PID control. | `spring`: soft springs overshoot, stiff ones track |
| `belt_axis` | A NEMA 17 stepper on a GT2 belt moving a 0.5 kg carriage 200 mm, with a limit switch and end stops. | `move_speed`: command it too fast and it loses steps, then stalls |
| `gripper` | An N20 gearmotor turning a Tr8×2 lead screw, squeezing a soft object; a load cell reads the grip. Power is cut at 1.5 s and the self-locking screw keeps holding. | `screws`: T8×8 and ball screws grip harder but relax after power-off |

## Things to try

- **Rover:** swap a gearmotor for the N20 (same interface: **Show alternatives**), halve
  the wheel `grip` and floor the throttle to watch wheelspin, or set the slope force.
- **Drone:** drop `kd` on the autopilot and watch it oscillate; raise the weight
  until the motors saturate; look at a winding's temperature after a long hover.
- **Arm:** remove the spring (connect the gearmotor straight to the link) and compare
  how a stiff joint and a series-elastic joint handle the same target.
- **Stepper axis:** double the carriage mass, then find the fastest move that still
  lands at 200 mm; the stepper's datasheet shows why (holding torque, stiffness).
- **Gripper:** swap the screw for `ball_screw_1204` and compare the force after
  power-off; replace the switched supply with a PID force loop on the load cell.

## Presets used

Named, representative parts (catalogue-style values, marked as estimates in
every parameter's provenance) published to `library/systems` by
`cargo run -p sim-runtime --example build_robot_kit`: `n20_gearmotor_100`,
`gearmotor_37mm_30`, `nema17_stepper`, `drone_unit_2212`,
`harmonic_drive_100`, `ball_screw_1204`, `lead_screw_t8x8`, `lipo_3s_2200`,
`hobby_servo_mg996r`. They snap onto anything with matching ports, and
same-interface presets swap for each other.

## Honest limits

Values are representative, not measured parts. Robots are one-dimensional
(the rover drives straight, the drone climbs vertically). The drone's
propellers ignore ground effect and blade dynamics; the stepper's driver is
an ideal current source (no torque drop at speed from back-EMF). The tests
(`cargo test -p sim-runtime --test robot_kit`) pin the behaviour described
above.
