# Composition and execution: one model for every system

Status: implemented 2026-10-04. "What runs end to end" and "Limits" at the
end say what is proven and what is not.

A system is one hierarchical composition of four kinds of instance,
assembled, validated, instantiated and executed by the same machinery
whether it is a thermostat, a battery pack or a walking robot:

| Instance | What it is | How it runs |
| --- | --- | --- |
| **Element** | A registered component with equations (`sim_core::BehaviorRegistry`): resistor, thermal mass, motor unit, articulated mechanism. Acausal physical ports and causal signal ports. | Compiled into the coupled DAE (`sim-compile` islands), stepped by the implicit integrator. Conservation holds at every physical connection. |
| **Subsystem** | A definition in the document: boundary ports, parameters, instances, nets. Placed many times, shared like a linked CAD component. | Flattened: its elements join the model; its path (`controls/thermostat`) is kept for selection, plots, results and evidence. |
| **Generated** | An assembly a registered generator builds from a source file. The `robot` generator turns a `.simrobot.json` into its mechanism, motors, drivers, sensors and thermal paths (`sim_runtime::robot_generator`). | Flattened like a subsystem; the document records its port signature, the flattener rebuilds it from the source and refuses a source that no longer offers those ports. |
| **Block** | An executable implementation (an FMI 3 FMU, a host-supplied controller) with typed signal ports, parameters, a clock, delays, a deadline and a lifecycle. | Executed by the runtime's block scheduler at committed clock ticks between integrator advances; never inside a residual, a Newton iteration, an event search or a rejected step. |

Physical connections (acausal ports) join conserved quantities and become
equations. Information connections (signals) carry values. A block touches
the physics only through signals: its outputs drive the plant through a
zero-order hold, its inputs are sampled from the committed plant state.

Code: `sim_core::block` (declarations, the `BlockImplementation` trait, the
shadow element), `sim_compile::blocks` (the scheduler),
`sim_compile::Runtime::{advance, bind_block, bind_coupler, snapshot}`,
`sim_system` (document schema `sim.system/2`, `InstanceKind::{Generated,
Block}`, `flatten_with` and the `Generator` trait), `sim_fmi` (the FMU
implementation), `sim_runtime::{system_blocks, robot_generator,
system_evidence, composition_examples}`.

## Why blocks are not elements

A controller is code with side effects and state outside the numerical state
vector. Evaluated inside residuals it would run once per Newton iteration and
per rejected trial step, and its state would not roll back with the
integrator. So executable code is never a `Behavior`. The coupled model sees a
block as a *shadow element* (`block`, `sim_core::BLOCK`): signal inputs that
only read, and signal outputs equal to held states with zero rate. The runtime
owns the implementation and writes the held states at ticks (`set_many`: one
batch, one consistency re-solve per island touched; the re-solved state is
retained as that instant's observation so algebraic values stay observable).

Native physical components stay elements with equations. Acausal
composition (tight coupling, conservation, implicit solve) and FMI
co-simulation (opaque code advancing itself between communication points)
are complementary: the first builds the physics, the second brings imported
executable code to it.

## Time semantics

All times are simulation time in seconds. A run starts at `t0`.

**Clock.** `Clock::Periodic { period T > 0, offset φ ≥ 0 }` ticks at
`tₖ = t0 + φ + k·T` (on the absolute grid, never by repeated addition);
`Clock::Times { times }` ticks at `t0 + times[k]` (a recorded acquisition
schedule, strictly increasing; no tick after the last). Ticks of all blocks
are merged; at an instant only the blocks whose clock ticks there execute.

**Physics stepping.** `Runtime::advance(d, h)` cuts the interval at every
tick: islands advance to the tick (a shorter last step lands on it), commit,
then the due blocks run. A tick exactly at the end of `d` runs in that call.
Step-size retries (`retry_halvings`) happen inside one segment between ticks,
so a controller is never called for a step that is later rejected. Physics
events inside islands (switching, contact) are the integrator's and never
call blocks. `advance_to_event` is refused for a model with blocks.

**One tick.** At an instant `t` with due blocks `D`:

1. *Apply what was decided before `t`.* A block without feedthrough (FMI
   Co-Simulation) applies the outputs its previous step computed: its
   values at `t`. A feedthrough block with an output delay of `d` samples
   applies what it computed `d` ticks ago.
2. *Initial outputs.* A block without feedthrough at its first tick is
   initialised and its initial outputs apply. Every such block reads the
   other blocks as they stood after step 1.
3. *Execute in dependency order.* Blocks in `D` run in topological order of
   same-instant edges (a producer with feedthrough and no output delay). A
   block's plant inputs are the committed state at `t`, before any write at
   `t` (sample, then update); its block inputs are the producers' applied
   outputs. A cycle of same-instant edges is an algebraic loop, refused when
   the runtime is built, naming the blocks.
4. *Write.* Every changed output is written in one batch.

What a consumer reads from a producer is therefore fixed by the connection,
never by the order the blocks were declared in: the producer's output of
this tick over a same-instant edge, its output of `d` ticks ago over a
feedthrough edge delayed by `d`, the result of its previous step over an
end-of-step edge. In step 3 only same-instant producers change their
signal, and the order puts each before its consumers.

**Inputs.** Sample and hold; `input_delay` whole samples (before the line
fills, the oldest sample held).

**Outputs.** Zero-order hold. Before a block's first write its outputs are
their declared start values; a block whose first tick is after `t0` must
declare one for every output (refused otherwise).

**First tick.** `initialize(t, dt)` (FMI: enter initialization mode with
`startTime = t`, set inputs, exit, read outputs); its outputs apply at `t`.
A block without feedthrough is initialised in step 2 (a block input reads
the producer as it stood before this tick's executions) and then takes its
first step from `t` in step 3 with this tick's inputs (its result applies at
the next tick); later ticks take one step each, of the interval to the next
tick.

**Adaptive advances.** `advance_adaptive` cuts at ticks like `advance`. A
segment shorter than the smallest step asked for (a tick just ahead, or the
end of the advance just past one) is taken as one step of its own length;
bounds with `h_min > h_max` are refused.

## Validation before execution

Refused when the model is built or edited, naming the block, the port and
the file:

- an input with no source, or a connection between a block signal and a
  signal of another quantity (exact match; no wildcard, no conversion) —
  checked when the document is edited (`Resolver::check_net`) and again when
  the model compiles (`CompileError::BlockSignalKind`);
- an algebraic loop among blocks;
- a clock with a non-finite or non-positive period, a negative offset, an
  unordered schedule; a missing start value for a late first tick;
- an implementation whose interface (names, quantities, feedthrough) differs
  from the block's declaration;
- for FMUs, everything in "FMI 3 profile" below; for a generated assembly, a
  source that no longer offers the recorded ports.

## State, reset, checkpoints

Each block instance owns its implementation instance; nothing is shared. An
FMU that declares `canBeInstantiatedOnlyOncePerProcess` cannot have a second
live instance (refused while the first exists). Every session build makes
fresh instances (`ModelSource::build` → `system_blocks::bind`), so a reset is
a new, reproducible run (tested bit for bit).

`Runtime::snapshot` includes the scheduler's state (next tick, pending and
applied outputs, delay lines) and each implementation's own state
(`BlockImplementation::checkpoint`): an FMU with `canGetAndSetFMUState` and
`canSerializeFMUState` serializes its state; a host coupler declared
stateless needs none; anything else makes `snapshot` refuse
(`RuntimeError::Unsupported`) rather than return a checkpoint that would not
restore the run. `BlockState::to_numbers` puts it in numeric snapshot formats.

## Failures

A fault stops the run with `RuntimeError::Block { block, time, message }`
and is sticky: an implementation error status (with the FMU's log), a
`fmi3Fatal` (no further calls, not even free), a non-finite output, an output
outside its declared range (an FMU's `min`/`max` become port ranges), a
missed deadline (`deadline_s`, wall time per call), an FMU asking to
terminate, return early or handle events. Every implementation is terminated
and freed when the runtime is dropped, after a fault included. Physical
safety limits are elements in the plant (saturations, servo firmware, fuses),
so they hold whatever a controller outputs.

## FMI 3 profile

Supported: **FMI 3.0 Co-Simulation**, fixed communication step equal to the
block's clock interval, no event mode (`eventModeUsed = false`,
`earlyReturnAllowed = false`, no intermediate update). Scalar `Float64`,
`Float32`, `Int8…UInt64` and `Boolean` variables with causality `input`,
`output` or `parameter` (variability fixed or tunable, set before
initialisation). Integer inputs are rounded and range-checked; a Boolean
input is true at 0.5 and above.

Why Co-Simulation and not Scheduled Execution: Co-Simulation is what FMI 3
exporters produce (Modelica tools, Simulink, the Reference FMUs);
Scheduled Execution FMUs are rare. Its communication step (outputs at the
end of the step) makes every block→plant and block→block edge loop-free by
construction. Scheduled Execution (clocked partitions activated by this
scheduler, zero-time tasks) fits the same scheduler and is the natural next
profile; it is not implemented.

Units: a port's quantity comes from the variable's unit (or its
`declaredType`'s): SI base-unit exponents from `<UnitDefinitions>` must equal
the quantity's, with factor 1 and offset 0. Where exponents are shared
(W: power or heat flow; J vs N·m) the variable's `quantity` attribute or the
unit's spelling decides, else the block must state the port's kind
(`system_add_fmu {kinds}`). A variable with no unit carries only
dimensionless values.

Refused by name before execution: FMI 1/2; Model Exchange-only or Scheduled
Execution-only FMUs; `needsExecutionTool`; clocks; arrays; `String`/`Binary`
ports; structural parameters; unknown variable names; a missing binary for
this platform (the error lists the platforms present); a second instance of
a once-per-process FMU; an archive whose SHA-256 differs from the one the
model recorded.

Implementation: the C ABI is written from the standard's headers
(`sim_fmi::abi`; headers vendored under `examples/fmi/include`, 2-clause
BSD), loaded with `libloading`; `modelDescription.xml` is read leniently with
`roxmltree` (`sim_fmi::description`). The maintained `fmi` crate (0.8) was
evaluated first and not used: its strict schema rejects the standard `unit`
and `quantity` attributes on variables, so FMUs from common tools fail to
load; `fmi-sys` needs libclang at build time for an ABI of a dozen functions.

Fixtures: `examples/fmi` (see its README) — a hysteresis thermostat and a
joint controller in C on a small Co-Simulation scaffold, packed into FMUs by
`sim_fmi::pack` / `sim-fmi pack DIR OUT.fmu`.

Not claimed: hardware fidelity or deployment. An FMU proves the controller
code runs against this model; firmware on a microcontroller (ESP32 timing,
fixed point, drivers) is a hardware adapter's concern.

## Robots

A robot is a generated assembly (`generator: robot`, source a
`.simrobot.json`) built by `physical::assemble`, the same code Robot mode
runs. Composed into a system its boundary is:

- `supply_p`, `supply_n`: the motor bus (electrical) — a battery or supply
  (with the generator parameter `own_supply = 1` the robot keeps the supply
  its model defines and offers no bus);
- `ambient`: the thermal environment the windings, cases and mounts shed
  heat to (`own_ambient = 1`: the model's own fixed ambient instead);
- inputs `<joint>.target` (servo setpoint) or, with `driver_control`,
  `<motor>.duty` (H-bridge duty);
- outputs `<joint>.angle`, `<joint>.speed` (encoders, tachometers), `imu.*`,
  and with driver control `<motor>.current|torque|speed`.

The names are the controller contract's. Robot mode (`PhysicalRobot::build`)
assembles with its own battery or supplies and ambient and puts one host
controller block on the signals; systems connect whatever they like. The
articulated element still receives its model through a process-local handle
(`register_model`); its value is never recorded (evidence fingerprints use
the document and the source file's bytes).

A generator may hand the host a handle on what it built
(`Generated::detail`, kept in `Flattened::generated_details`): the robot
generator's is the assembly, so host code can measure the robot inside the
compiled system (`PhysicalRobot::attach`) with the code Robot mode reports
with. When a source file changes its ports (a re-exported model with another
joint), the `refresh_generated` command records the new signature; a
connection to a port that is gone is refused by name.

## Robot projects

A robot project (`*.robot.json`, Design → Model → Test → Learn → Make) tests
its robot as it is composed in the project's system (`<name>.system.json`, a
`sim.system/2` file like any other). The first test makes it: the model as a
generated robot with its own supply and ambient, run with the robot
assembly's step and Newton settings. Controllers (FMU blocks), a battery or
thermal parts are added to it in Build mode.

`sim_runtime::acceptance::run` runs that system on the path every system run
takes — flatten with the generators, compile, bind FMU blocks, schedule — so:

- controller blocks in the system run as they are; nothing replaces them;
- the test's trajectory commands only signal inputs the system leaves open
  (a joint's servo target when nothing drives it, or a controller's own
  setpoint named `instance.port`), through a test-bench block on the same
  scheduler at the robot model's control period and latency. Commanding an
  input the system already drives is refused. Open robot inputs the test
  does not name are held (a servo target at the model's control target);
  the report lists commands, holds and the controllers that ran;
- the criteria are the robot's own (reach, tracking, torque against stall,
  winding temperature, bearing and yield margins, falls, limits, printed-part
  strength), read from the compiled model by `PhysicalRobot::attach`. Every
  sampled series has one value per sample; tracking is judged against the
  test's command for the joint, or the target a controller sent it.

The report carries the same fingerprint as system evidence (the system's
physics hash, the SHA-256 of every file read — the robot model, FMUs, and
for part strength the CAD file and the print registry — the run settings and
the test). `acceptance::standing` compares it with what a run would be now:
`project_state.test.standing` is current, or stale with what changed, and a
stale pass neither completes the Test step nor allows Make.

## Systems, tests and evidence

A system file (`sim.system/2`) records block and generated instances with
their interface or port signature, the FMU's path and SHA-256, block timing,
and FMU parameters as ordinary parameter bindings (so they can inherit a
parameter of the enclosing definition). Paths are relative to the file.
`sim_runtime::system_builder::compile_at` flattens with the host's generators
and the session binds every block; a host block is refused by a host that
does not supply it.

Tests (`SystemDocument::tests`) are requirements on observables by readable
key (`thermometer.temperature`), reduced over a window and bounded. A test
runs the system as composed — its own controllers, no substitute, nothing a
controller could not read. `system_evidence::assess` judges each requirement
pass, fail or not assessed (missing observable, a run that stopped before
the window, non-finite) and binds the evidence to a fingerprint: the
document's physics hash (no display-only content), the SHA-256 of every file
the run read (FMUs, generator sources), the run settings and the test.
`standing` reports a test not assessed, current, or stale with what changed.

Build mode: `system_add_fmu`, `system_add_robot`, `system_inspect_fmu`,
`system_test`, the `set_block_timing` and `set_test` commands, and
`system_guide` (also `GET /v1/system_guide`); `system_state.composition`
shows blocks, tests, standing and evidence.

## What runs end to end

- `cargo run -p sim-runtime --example composition_demo -- DIR` builds the
  fixture FMUs from C, authors two systems through the command path
  (`sim_runtime::composition_examples`) and runs them headlessly: a room held
  in its band by the thermostat FMU, and the rover (generated robot, its own
  battery, an enclosure on its thermal port, one joint-controller FMU per
  drive wheel, two independent instances) for 2 s. Either file opens in the
  app (`sim-spatial --system DIR/rover.system.json`) and runs there.
- In the app: `target/debug/sim-spatial` then
  `python3 examples/composition/thermostat_over_rest.py [DIR]` performs the
  Build-mode workflow over REST (the same handlers as the controls): an
  empty system, a room from thermal parts, `system_inspect_fmu`,
  `system_add_fmu`, wiring, `set_block_timing`, a saved test run on a
  background thread (evidence current), then the live run. The library tab's
  "Add an FMU block" / "Add a robot" fields, the inspector's block clock and
  the Studies tab's Tests section are the same actions by hand.
- Headless: `sim-system check|run|test FILE …` resolve generated robots and
  FMUs against the file's directory.
- Tests: `sim-phenomena/tests/blocks.rs` (scheduler semantics: feedthrough
  and end-of-step timing, delays, offsets, schedules, units, interface
  mismatch, algebraic loops, faults and termination, deadlines,
  checkpoints), `sim-fmi/tests/fmi.rs` (import, inspection, regulation,
  independent instances, reproducible reruns, timing, exact units, every
  refusal, faults and cleanup, artifact identity, FMU state checkpoints),
  `sim-runtime/tests/composition.rs` (the two systems saved, reloaded,
  flattened with paths; parameter inheritance; a block inside a subsystem;
  wiring refusals; changed artifacts and host blocks refused; evidence
  current, stale on a model, artifact or test change, not assessed, failed),
  `sim-runtime/tests/robot_acceptance.rs` (a robot project's test through
  its system: alone under the test bench, with controller FMUs that are
  never overridden, stale on a test, system, model or judged-file change, a
  re-exported model refreshed).

## Limits

- Only FMI 3 Co-Simulation; no Scheduled Execution, Model Exchange, clocks,
  arrays, strings or event mode.
- Integer and Boolean FMU ports carry dimensionless values only.
- A host block runs only where its host binds it (`system_blocks::bind_with`:
  a robot test binds its test bench; Build mode's live run binds
  `drive_input`, `sim_runtime::teleop`). Headless runs refuse a
  `drive_input` block by name.
- A robot test judges one robot: a system with several generated robots is
  refused. Robot mode runs a project's composed system when the open model
  belongs to one (`teleop::compose_robot`); a model without a project runs on
  its own (`PhysicalRobot::build`).
- Jogging a joint whose target a controller drives needs a free setpoint
  input on that controller (`BlockPort.setpoint`); otherwise the jog is
  refused by name (`PhysicalRobot::jog_refused`).
- Printed-part strength loads a link that is driven and also carries a
  further joint with both load cases, which overstates.
- A system with a robot travels as a whole: the robot model is a
  content-addressed resource of the model world (`sim_core::resources`, a
  52-bit BLAKE3 key), installed into a process cache when compiled.
- Blocks do not join islands: each island that reads a block output holds
  its own copy, written at every block tick (`sim_compile::blocks`).
- The browser build runs systems without FMU blocks only.
