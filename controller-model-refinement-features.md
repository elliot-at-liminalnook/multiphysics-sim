# Controller design and measured model refinement

Next-phase feature proposal · September 13, 2026

Full-drive follow-up: the user authorized 100% PWM and coordinated rapid reversals.
The integrated device-clock firmware and Rust capture path are implemented and
verified in software; the image is built and passes 50 MHz timing. It has been loaded into SRAM and its live profile verified; powered motor
commissioning remains pending. **No new full-drive motor measurements or model promotion**
have occurred. See the [full-drive implementation status](examples/actuators/hx30hm/hardware/2026-09-14-full-drive/README.md).

Latest September 14 update: [thirteen further physical bench captures and per-motor refinement](examples/actuators/hx30hm/hardware/2026-09-14-unloaded-refinement/README.md)
retain three fresh all-nine confirmation profiles. Candidate prediction error fell
21.6% overall, but motor6 regressed and no motor passed the original accuracy
gate. Shared-supply simulation now runs all nine axes in one electrical circuit;
its electrical parameters remain uncalibrated. Tests used at most 10% PWM, so
higher-drive/full-speed stress testing remains a distinct commissioning milestone.

September 14: actual FPGA speed-ramp/reversal trials and their matching simulation
are now retained in the [physical response report](examples/actuators/hx30hm/hardware/2026-09-14-speed-response/README.md).
Speed and acceleration analysis is shared with the native viewer. Uneven measured
speed and insufficient settled tails leave complete response times unvalidated;
no maximum-speed or accepted motor-model claim is made.

Implementation is underway. Fresh measurements now include repeated low-duty
responses and our PWM controller on all nine bench motors. Initial model
predictions still miss the comparison limits. See the
[current implementation and acceptance status](examples/experiments/CONTROLLER-REFINEMENT-STATUS.md).

**Latest bench milestone:** the controller now executes on the FPGA and has driven
all nine motors together through changing-speed trajectories. The same controller
also runs in Rust simulation. Measured accuracy remains outside the declared gates
on the harder coordinated tests; model acceptance is still unfinished. Faster
onboard timing, per-motor refinement and calibrated electrical measurements remain
part of the feature's intended outcome. The native experiments panel now reviews
these FPGA recordings, compares replay and simulated feedback, and retains bounded
fits with per-device models. Editable FPGA gains and motion profiles now run in
simulation and export as new experiment plans; physical accuracy acceptance,
per-axis gains and autonomous timing remain unfinished.

**Agreed control interface: low-level signed PWM.** Our controller owns the
feedback law that generates duty commands. Existing PWM recordings are the primary
starting evidence for refining the plant beneath that controller.

**Available measurement setup: unloaded motors on the bench.** Start with one
identified motor at a time, then check repeatability across the available units.

## Outcome

Use the Rust systems viewer to design a controller against a model whose accuracy
is measured, improve that model using real motor and joint behavior, and carry
the accepted model into the full robot simulation and eventual Rust CAD viewer.

Accuracy is a property of a tested behavior and operating range. The tool should
answer: **How accurately does this model predict this controller's behavior on
this assembly, under these conditions, and what evidence supports that claim?**
Passing software tests or successfully producing simulated traces does not answer
that question.

The first acceptance target is unloaded motor response and our own signed-PWM
feedback controller on the bench. Known-load joint, leg and quadruped validation
follow when those setups are available. Both the initial physical setup and
control interface are agreed. This document does not start a hardware experiment.

## What exists, and what the evidence does not yet establish

- The current panel reviews two PWM studies, reruns shared physical components,
  compares candidates, preserves validation splits, and saves results.
- The [measured position-mode report](examples/actuators/hx30hm/hardware/2026-09-11-nine-servos/motion-report.md)
  contains additional individual and concurrent motion recordings for nine
  servos. These are not yet supported by the panel's PWM-only comparison adapter.
- Those recordings used free output shafts. Their report explicitly identifies
  loaded torque, friction, backlash and inertia as remaining measurement needs.
  Interrupted and variable-supply runs must keep their distinct conditions.
- Shared Rust controller interfaces and a sampled servo-firmware model exist.
  The current pulse comparison bypasses that firmware model. Its presence in the
  library is not evidence of parity with the Hiwonder's internal controller.
- The joint model already represents friction, stiffness and drive backlash,
  including provenance. Their availability is not evidence that the parameters
  describe the assembled robot accurately.

## 1. Define what the controller can actually observe and command

Each controller experiment declares its command interface, measured signals,
update rate, timing conventions, limits and startup state. Use the same controller
source and settings in simulation and eventual hardware execution, with different
adapters for the simulated and measured system.

| Control interface | Model behavior that needs validation |
| --- | --- |
| Signed PWM commands — selected | The drive, motor, transmission, load and sensing response beneath our own feedback controller |
| Servo position/speed commands — separate reference data | The complete servo response, including internal trajectory shaping, saturation, delay, sensing and motor/load dynamics |

The controller's action is normalized signed duty, with explicit limits and a
recorded update schedule. Desired position or motion is an input to our controller;
the controller computes PWM from the available feedback. PWM duty is not a
measured torque or current command.

The simulated controller must receive the observations available on hardware,
including encoder resolution, measurement age and bus timing. Exact simulated
velocity or torque must not silently replace signals unavailable to the physical
controller. Any estimated velocity uses the same estimator and sample history.

PWM-mode driver behavior, saturation, internal protections and braking/coasting
semantics still need validation. Selecting PWM does not establish that all device
firmware effects disappear. Position-mode recordings remain separately labelled
evidence of behavior with the servo's internal position control active.

Controller design should expose source or a named controller, editable parameters,
desired trajectories, limits and the signals available to feedback. Rust and
Rhai-backed controllers use the shared runtime. New physics and controller
components belong in the library rather than in the viewer.

## 2. Separate three kinds of comparison

The UI gives each run an explicit purpose:

1. **Model calibration:** freeze the recorded inputs, conditions and controller
   assumptions; change only the candidate plant model.
2. **Controller design:** freeze the selected plant revision; change the
   controller or its configuration.
3. **Closed-loop validation:** run a captured controller/experiment against the
   simulated and physical systems, preserving each system's own feedback and
   actual timing.

Replaying recorded commands tests response to those commands. Feeding a
controller recorded observations can test its calculations. Neither alone proves
that its simulated closed loop predicts its hardware closed loop. The tool must
keep these conclusions distinct.

## Electrical behavior and battery scenarios — added requirement

**Available bench measurements: servo telemetry only**, confirmed by the user.
Continue motion refinement with reported voltage and the raw current register.
Calibrated supply/winding current, power and energy validation remain pending
additional measurement capability; simulations must label those channels as
predictions rather than measured validation.

Controller design, simulated experiments and measurements must account for voltage,
current, power and energy, including operation from a battery. Keep supply/battery
current separate from motor winding current; PWM and driver losses mean they are
not interchangeable. Record the measurement location, sign, units, timing, sensor
calibration and uncertainty for each channel. An uncalibrated servo current register
is not a measurement of battery amps or watts.

The experiment can declare a regulated supply or battery model, capacity and state
of charge, voltage response under load, auxiliary electronics load, and applicable
voltage/current/power limits. Reuse the shared circuit and battery components in
headless, interactive and robot execution. Battery parameters and discharge curves
remain hypotheses until matched to evidence. Show depletion, overcharge and model
limits explicitly rather than presenting an invalid continuation as a valid run.

Controllers may use declared electrical observations for voltage compensation and
sampled voltage/current/power protection. Simulation must reproduce the available
sensor resolution, delay and missing channels; it must not supply perfect winding
or battery measurements unavailable to the real controller. Sampled limits do not
replace the FPGA's independent hardware protection.

Reports show voltage sag, supply and winding current, draw/return power, consumed
and returned energy, state of charge where modeled, electrical limit violations,
and mechanical tracking together. Compare synchronized measured electrical traces
with simulated channels independently of motion accuracy. Missing current sensing
or calibration leaves current/power/energy accuracy unvalidated. Test controllers
at different charge levels and shared-load conditions, including the effect of
other motors drawing from the same source.

## 3. Add an accuracy report with explicit coverage

Every model revision shows what has been tested, what remains estimated, and
where prediction fails. Scope includes device and joint identity, motion range,
direction, speed, load, supply, temperature and controller mode.

| Behavior | Evidence the report should show |
| --- | --- |
| Position tracking | RMS, peak and settled error on unseen trajectories |
| Transient response | Delay, rise/release behavior, overshoot and settling, where acquisition timing supports them |
| Loaded response | Measured displacement/deflection and response under declared inertia, gravity and applied load |
| Reversal and small motion | Direction dependence, breakaway, clearance and hysteresis across repeated trials |
| Coupled operation | Changes when multiple actuators share supply and mechanical loads |
| Controller outcome | Whether simulated stability, tracking and limit behavior agree with measured runs |

Set numerical limits before evaluating reserved trials. Choose them from the
robot task's needs and the measurement system's resolving ability; do not invent
precision beyond the available instruments. Separate measurement uncertainty,
run-to-run variability and uncertainty in model predictions.

Track errors against limits, coverage and evidence quality instead of collapsing
everything into an unexplained percentage called “model accuracy.” Mark
out-of-range predictions as extrapolations.

## 4. Make experiments distinguish competing explanations

### Acceleration, braking and direction reversal — September 14 requirement

The motor model must predict how quickly the output reaches a requested steady
speed and how quickly it changes to a steady speed in the opposite direction.
Measure and compare these responses explicitly, alongside position tracking:

- From rest: command delay, 10–90% speed rise and time to settle at the declared
  speed. Define "up to speed" using a declared tolerance band and sustained dwell,
  rather than assuming an exact instantaneous 100% crossing.
- Reversal: time from the actual command to the zero-speed crossing, then to the
  settled opposite speed; report total reversal time and angular travel before
  reversal. Do not combine braking and opposite-direction acceleration into one
  unexplained score.
- Record sampled peak speed, overshoot and acceleration/deceleration estimates,
  preserving estimator window, encoder resolution, sample age and timing bounds.
  Use the same observable feedback/estimator in hardware and simulation; do not
  compare perfect simulated velocity with coarse measured finite differences.
- Repeat both directions, multiple initial/target speeds and PWM levels, single
  motors and simultaneous operation. Retain motor identity, supply voltage,
  raw current, temperature and unloaded/loaded conditions for each result.
- Compare measured and predicted response intervals on reserved trials. Mark
  missing plateaus, insufficient dwell, clipped travel, unresolved crossings or
  inadequate sampling as unvalidated rather than inventing a response time.

These are condition-dependent consequences of motor/drive dynamics and the
controller, not a universal hardcoded acceleration or reversal delay. Refine the
shared motor model's inertia, torque/speed response, losses, drive/braking and
sensing behavior against this evidence while preserving physical bounds and
identifiability limits. Available unloaded, limited-duty bench trials cannot
establish maximum-drive or loaded reversal performance. "Full speed" must name
the tested speed and drive conditions; physical maximum-speed claims require
separate adequately sampled, commissioned tests within available shaft travel.

For the available unloaded bench, prioritize repeated low-duty onset trials in
both directions, pulse and trajectory response, reversal, and separately recorded
zero-PWM versus torque-off release. Record motor identity, starting angle, supply,
temperature, attached output hardware, commands and acquisition timing. Unloaded
means no intentional external load; it does not mean zero rotor or output inertia.

Then validate our controller on held-out position trajectories using signed PWM
and the same encoder-feedback estimator in simulation and hardware. Establish
the achievable observation/command cadence from recorded timing before selecting
the controller update rate. Existing open-loop recordings can calibrate response;
new recordings of our controller are needed to validate its physical closed loop.

These trials constrain observable unloaded dynamics and their repeatability.
They do not uniquely identify all electrical/mechanical constants or establish
loaded torque, transmission deflection, or assembled-joint accuracy. The accuracy
report should show those areas as unvalidated until suitable measurements exist.

Organize measurements around specific questions, with required conditions and
signals shown before a run is interpreted:

- Unloaded command steps and trajectories: command-to-motion response, directional
  differences and speed limiting.
- Known inertia and gravity loads: acceleration and holding behavior at multiple
  configurations.
- Small reversals under load: transmission clearance, compliance and friction.
  Where the servo encoder cannot observe motion beyond a belt or linkage, an
  independent joint/output measurement is needed.
- Release and braking: distinguish zero-command response, active braking and
  torque-off/coasting using separately identified experiment modes.
- Supply and simultaneous-actuator trials: separate actuator behavior from shared
  electrical effects.
- Repeated warm/cold trials: establish repeatability and condition dependence
  before interpreting differences as per-motor properties.

The library's existing waveform files are acquisition plans, not proof those
measurements have been taken. Existing recordings remain usable, with their
limitations visible. New measurement collection is a separately prepared step.

## 5. Help the user refine the model

Add parameter sensitivity views: show which observable behaviors change when a
parameter changes and which proposed experiments could distinguish parameters
that currently produce similar traces.

Allow bounded candidate fitting as an explicit next-phase extension to the
first version's manual editing. Freeze the tuning set, preserve plausible bounds
and physical constraints, and compare with untouched validation trials. Report
when several parameter combinations explain the data similarly; an optimizer
converging does not identify the true physical constants.

Use family-level parameters and per-device deviations where supported. Preserve
device identity so variation across the nine servos does not disappear into one
average curve. Avoid explaining sensor delay or firmware behavior by silently
changing physical inertia or friction.

## 6. Design controllers against uncertainty

A controller should be evaluated against the retained baseline, candidate and
plausible variations supported by the evidence, rather than just the single
best-fitting trace. Preserve tracking error, oscillation, saturation, failures
and sensitivity to timing and load.

Keep the controller-design score separate from model-prediction error. A
controller can improve even while the model remains inaccurate, and a more
accurate model may reveal that the current controller performs poorly.

## 7. Carry accepted evidence into the robot

Associate hardware IDs with stable CAD component/joint identities explicitly.
Capture the fixture, transmission, geometry, mass properties and initial pose
used by each experiment. Preserve this association when moving between schematic,
experiment and future Rust CAD views.

Offer a reviewable proposal to apply accepted physical properties back to CAD,
with units, source, uncertainty and tested scope. Keep experimental overrides
distinct until accepted. Validated component models must be used by the shared
robot runtime; the viewer must not maintain a separate physics implementation.

Revalidate at each level: motor → loaded joint → leg → quadruped. Assembly
coupling, structural compliance, power distribution and contact can introduce
errors that isolated-motor measurements do not cover.

## Proposed delivery order

| Increment | User-visible result | Evidence needed to call it complete |
| --- | --- | --- |
| PWM controller experiment definition | Select controller, trajectory, hardware-available observations, conditions and frozen model revision | A saved experiment reproduces signed-PWM controller behavior with explicit timing, limits and feedback estimation |
| PWM replay and feedback comparison | Compare recorded PWM response and our controller's simulated closed-loop response against their respective evidence | Command replay and closed-loop validation remain distinct; each retains its commands, observations and timing |
| Unloaded bench validation | Compare repeated motor-response trials and our PWM controller on unseen trajectories | Unloaded response and closed-loop prediction meet predeclared tolerances within measured timing and sensor limits |
| Accuracy and refinement tools | Coverage report, sensitivity, bounded fitting and uncertainty-aware controller comparisons | Reports distinguish identifiable parameters, overfitting, extrapolation and genuine held-out prediction improvements |
| Loaded-joint calibration — later setup | Compare measurements and candidate models for one identified joint/load | Repeated, adequately measured trials constrain the intended behavior; candidate evaluated on reserved trials |
| Assembly validation and adoption | Carry accepted revisions into leg/robot experiments and propose CAD updates | Shared runtime uses the captured revisions; new joint/assembly-level measurements verify the claimed accuracy |

The next concrete milestone is **a captured controller experiment on one unloaded
bench motor with an explicit accuracy target**, using our own signed-PWM feedback
controller. Task tolerances and measured acquisition timing determine the
remaining implementation details. Extremely accurate
full-robot behavior remains an evidence-driven objective, not a capability the
current panel can already claim.
