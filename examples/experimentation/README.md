# CAD and Rhai experimentation examples

Build the runner and generate the editable models:

```sh
cargo build --release --bin sim-experiment
cad/.venv/bin/python examples/experimentation/build_model.py
```

Open a generated `.rcad` file from `runs/experimentation-models` in the CAD editor.
The pendulum and two-joint examples use `system.rhai` and `controller.rhai` in this
directory. See [the workspace guide](../../cad/EXPERIMENTS.md) for run capture,
plots, replay, comparison, annotations and revision-checked API edits.

## Coupled electromechanical–thermal example

`electromechanical-thermal.rcad` contains the motorized pendulum plus a system
graph. Its inspector exposes the existing motor, winding storage, case, thermal
conductances and mount, and adds a housing temperature sensor. These are bindings
to the original native components; the graph does not instantiate another motor
or duplicate its thermal storage.

The case's **Body mass × specific heat** rule uses its modeled motor envelope,
ABS density, and an explicit 1000 J/(kg·K) specific heat. It treats that complete
solid as the case's lumped thermal mass. This is an illustrative, uncalibrated
approximation: real servo interiors, air gaps, mixed materials and spatial
temperature gradients require a more detailed model and measured parameters.

In **Experiments**, use `let assembly = cad("assembly");` as the system, the
supplied Rhai controller with `target1: 0.2`, and a one-second quick-check run.
Inspect `graph/housing.node.temperature` and `graph/sensor.temperature` in kelvin,
alongside winding temperature, motor current, joint tracking and CAD replay.
Select the motor body to filter its attached signals.

Try changing **Housing to air → conductance** from 0.2 to 0.4 W/K and rerun. The
mechanical CAD cache is reused and the housing ends cooler. Change the motor
geometry and rerun to change its derived capacity. The graph's CAD bindings use
body IDs, so renaming the motor does not break its connections. Each run retains
the original graph, derived inputs/values, scripts and component-to-body mapping.

## Measured acceptance contract

```sh
SIM_EXPERIMENT_REQUIRED=1 cad/.venv/bin/python -m pytest \
  examples/experimentation/test_workspace.py -q \
  --basetemp=runs/experimentation-acceptance
```

The coupled example checks:

- One native motor, reused native storage and sensor agreement at every sample.
- Independent case energy balance:
  `C ΔT = ∫[Gwc(Tw−Tc) − Gca(Tc−Ta) − Gcm(Tc−Tm)] dt`, within 2% relative or
  1e-7 J absolute error, using 0.5 ms steps and 5 ms recorded samples.
- Exact repeatability of complete traces and controller samples on a cached run.
- A 1.2× linear geometry scale produces 1.728× capacity within 1e-8 relative error;
  a rename retains bindings and graph signal identities; historical results become stale.
- REST/Python component edits affect cooling and retain mechanical cache hits.
- Actual Qt controls launch runs, select a baseline, change cooling through the
  inspector, compare the recorded traces, scrub CAD replay and create a comment
  bound to the motor, run, signal and selected time. Small absolute-temperature
  changes use an explicitly labelled reference offset in the plot; the sample
  readout retains the absolute temperature.
- A cached one-second coupled run finishes within 6 s and REST acknowledgement
  within 100 ms. CI enforces these with `EXPERIMENT_THERMAL_SECONDS` and
  `EXPERIMENT_ACK_SECONDS`; the main pendulum gate also checks UI heartbeat and
  worker/controller cancellation.

`thermal-acceptance.json` records measured errors, timings and run IDs. On the
Intel i9-9980HK/macOS 26.6 development host, the first complete API gate measured
0.0371% case heat-balance error, 58.3 ms acknowledgement and 1.649 s cached total
time. Those measurements establish this example's contract, not general
multiphysics accuracy or performance on arbitrary assemblies.
