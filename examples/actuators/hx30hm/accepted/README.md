# Accepted HX-30HM actuator families

`registry.json` is the single source of motor values for the quadruped. It
names each accepted family with its content hash (BLAKE3 of the canonical
JSON) and assigns a family to each joint role:

| Joint role | Family | Basis |
|---|---|---|
| Foot servo output (knee) | `hx30hm-knee-measured` | Campaign `campaign-1790182543587`, servo 1 |
| Hip servo output (belt/hip) | `hx30hm-hip-measured` | Same campaign, servo 3 |
| Worm servo output | `hx30hm-provisional` | Estimates; the worm has not been characterized |

Values from the campaign:

| | Knee | Hip | Provisional |
|---|---|---|---|
| Back-EMF / torque constant (V·s/rad) | 0.01213 ± 0.00101 (derived) | 0.01367 ± 0.00069 (derived) | 0.00973 (estimated) |
| Gear friction (N·m) | 0.019 ± 0.021 (derived) | 0.011 ± 0.023 (derived) | 0.025 (estimated) |
| Full-drive speed at the campaign supply (rad/s) | 4.93 at 12.4 V | 4.43 at 12.5 V | — |
| Full-drive speed at the simulated 11.1 V (rad/s) | 4.40 | 3.91 | 5.46 |
| Measured acceleration envelope (rad/s²) | 21.9 | 21.2 | — |
| Coast deceleration (rad/s²) | 1.9 | 1.3 | — |
| Low-speed friction / breakaway (drive fraction) | 0.050 / 0.072 | 0.057 / 0.074 | — |

Each family's `limitations` list what is and is not identified. Only the
back-EMF constant × gear ratio is measured; the 200:1 ratio, resistance,
efficiency and inertia remain estimates.

## How the values reach every system

The rule is one source, derived on use, hash-checked:

1. **Promotion.** `promote_actuator_family CAMPAIGN_DIR AXIS BASE NAME OUT`
   derives a family from a campaign report (`sim_runtime::acquisition::actuator_promotion`).
   It writes a derivation record with every input, equation and intermediate value.
   Accepting the family means adding its path and printed hash to `registry.json`.
2. **Registry.** `sim_runtime::actuator_registry::Registry` loads the families
   and rejects any whose content no longer matches its accepted hash. `apply`
   binds every profiled motor to its role's family. `check` fails when a model
   carries anything else.
3. **Limits derived from physics.** `actuator_registry::joint_limits`
   resolves each motor's profile and derives its no-load speed, full-drive
   speed and stall torque at the simulation's own supply voltage. The
   measured envelope supplies the acceleration limit.
4. **Gait search.** A recipe with `actuator_limits` calls
   `Recipe::sync_actuators` on load. It applies the registry to both robot
   copies and derives:
   - the reference-speed screen;
   - the planner's actuator model (`no_load_speed`, `stall_torque`);
   - the governor search bounds.

   The hosts `compare_gait_search`, `prepare_gait_candidate`,
   `audit_gait_screens` and `prepare_contact_motion` all sync. The study
   records `actuator-provenance.json`, and the synced values enter its
   identity hash. Changing an accepted family therefore makes a running
   study's identity stale rather than silently mixing values.
5. **Hardware playback.** The calibration server plays gaits on the leg with
   the same Rust gait sampler as the browser (`gait_playback`, exposed to the
   WASM worker as `GaitPlayer`). Playback speed is capped by the controller's
   speed ceiling and the belt-safe acceleration.
6. **Tests.** `cargo test --release -p sim-runtime --test actuator_registry`
   covers:
   - the hashes and roles;
   - that each measured family re-derives exactly from its cited campaign
     report (no hand edits survive);
   - that the gait recipe's screen and governor bounds equal the derived limits;
   - that a stale family copy is rejected.

Historical study directories keep the families they ran with, as evidence.
They are not rewritten.

Tools:

```sh
cargo build --release -p sim-runtime --example actuator_registry --example promote_actuator_family
target/release/examples/actuator_registry hash FAMILY.json
target/release/examples/actuator_registry limits examples/actuators/hx30hm/accepted/registry.json SCENE-OR-CONFIG.json
target/release/examples/actuator_registry apply  examples/actuators/hx30hm/accepted/registry.json IN.json OUT.json
target/release/examples/actuator_registry check  examples/actuators/hx30hm/accepted/registry.json FILE...
```

## Not yet synced

- **Browser walking bundle** (`browser-control-400hz`): it still uses the
  per-role bench-fit families 10/11/12 with its 400 Hz controller variant.
  Moving it to the registry needs its prepare step rebased on `apply` plus
  an explicit controller-cadence override, then re-verification.
- **CAD `.rcad` archive**: the families are not yet authored into a CAD
  revision (`Ops.set_actuator_profiles`). Scenes get them through the
  registry.
- **Hardware controller**: the leg runs the calibration bridge's host feedback
  loop, not the FPGA fixed-PD law the simulation uses.

## Knee check against the motor model

`knee-dyno-check-*.json` runs the shared single-axis dynamometer
(`characterize_actuator`) at the campaign's open-loop steps (±0.15 and ±0.3
drive, 12.5 V, measured gravity load). The knee family's steady speeds are
within 9% RMS of the measurements, against 19% for the provisional values.
The remaining error is mostly the gravity asymmetry between directions,
which is modelled as one constant load.
