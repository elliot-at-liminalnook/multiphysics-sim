# Fixture FMUs (FMI 3.0 Co-Simulation)

Two small controllers written in C, used by the FMU import tests
(`crates/sim-fmi/tests/fmi.rs`), the composition tests
(`crates/sim-runtime/tests/composition.rs`) and the composition demo
(`docs/architecture/composition.md`, "What runs end to end").

| Model | What it does | Ports |
| --- | --- | --- |
| `thermostat/` | Hysteresis heater control: on below `setpoint − band/2`, off above `setpoint + band/2`. `fail_after` / `nan_after` (s) make it fail a step or output NaN from that time: fault fixtures. | in `temperature` (K); out `heater_power` (W, quantity HeatFlowRate, min 0), `heating` (Boolean) |
| `joint-controller/` | One joint's outer loop: a sine reference `offset + amplitude·sin(2π·frequency·t)` plus integral trim `ki`, clamped to `limit`. | in `angle` (rad); out `target` (rad, ±3.2) |

Each directory holds `modelDescription.xml` and `sources/*.c`. The models
share `include/cosim_scaffold.h` (a minimal Co-Simulation implementation:
all state in the instance, FMU state save/restore by copying it, no event
mode, no Model Exchange). `include/fmi3*.h` are the standard's headers
(Modelica Association, 2-clause BSD, licence text in each file).

## Build

The binaries are platform-specific, so they are built, not committed:

    cargo run -p sim-fmi --bin sim-fmi -- pack examples/fmi/thermostat target/fmus/thermostat.fmu
    cargo run -p sim-fmi --bin sim-fmi -- inspect target/fmus/thermostat.fmu

`pack` compiles `sources/*.c` with the system C compiler (`cc`, or `$CC`;
`-std=c99 -O2 -fPIC -shared -Wall -Werror`, the headers on the include path)
into `binaries/<arch>-<os>/<modelIdentifier><dll suffix>` and zips it with the
model description and sources (entries in a fixed order with a fixed
timestamp). It prints the archive's SHA-256, which a system file records for
the block that uses it. Supported build hosts: macOS and Linux.
