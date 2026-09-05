# Domain and connector roadmap

What to add to the library next, in the order the dependencies suggest, and
for each one the single scenario that proves it earned its place. Every
entry follows the shape of `surprise-tests.md`: one knob, a qualitative
change, a published number, a falsifier.

## The rule that governs all of it

**Contribute to the library; never write a one-off.** A phenomenon is only
proven when it falls out of the compiled system, so a scenario may author a
`ModelWorld` and read the `StateStore` — nothing else. If a scenario needs
an equation the library does not have, the equation goes into a domain crate
as a registered behavior with typed ports, and the scenario uses it like any
other part. If a scenario needs a numerical facility (a new constraint
kind, a noise process, a different discretisation), the facility goes into
`sim-core`/`sim-compile`/`sim-dynamics` and is available to every domain.
Reference calculations (stability boundaries, describing functions,
Poincaré maps) take the compiled system as input.

Two tests of whether something is a library contribution rather than a
one-off:

1. Could a second, unrelated scenario use it unchanged? If not, it is too
   specific — generalise it or split it.
2. Does it introduce a port kind, lane, or event pattern that other domains
   should share? If yes, define it in `sim-core` first.

The suite's history is the argument: the escapement, the sampled
controller, the seated valve and the stick–slip contact each began as
scenario code, and each one was wrong in a way that only showed up once it
became a library element compiled through the same path as everything else
(duplicate ports, algebraic-variable alternation, zero-guard events,
rate-average sensors). The library is where mistakes get found.

---

## Planned authoring layer: Rhai system definitions

User direction: support `.rhai` scripts that specify simulations and systems
by composing components from the Rust simulation library. This is planned
work; no Rhai runtime or dependency has been added yet.

The integrated delivery plan is [CAD + Rhai experimentation workspace](experimentation-roadmap.md).

- [ ] Add a Rust-hosted Rhai adapter (proposed crate: `sim-script`) exposing
  registered component construction, parameter assignment, typed port handles,
  connections, and reusable subsystem functions. Build the existing
  `ModelWorld` and compile it through the existing simulator pipeline.
- [ ] Keep physical equations, component implementations, and numerical solving
  in the Rust library. Evaluate system-construction scripts before simulation;
  script edits should not require rebuilding Rust components.
- [ ] Allow a script to incorporate a CAD-derived physical assembly and compose
  its surroundings: power, actuators, sensors, loads, and controller bindings.
  Preserve CAD part IDs in the resulting model for results and annotations.
- [ ] Integrate scripts with the planned experiment workflow: retain the entry
  script, imported modules, parameters, CAD revision, controller revision,
  seed, solver settings, and library/binary identity in each run's inputs.
  Both native Rust and Rhai authoring should produce the same run contract.
- [ ] Provide component/port discovery and diagnostics mapped to script source
  locations, including unknown components, invalid parameters and incompatible
  connections. Keep a failed build from replacing the last valid model.
- [ ] Prove parity against a small Rust-authored system using the same compiler,
  initial state, seed and solver configuration; declare trace tolerances and
  measure script-evaluation overhead separately from simulation time in CI.

System construction and sampled control use separate adapters. The integrated
experiment plan includes Rhai sampled controllers through the existing `Coupler`
contract with simulation-time semantics, preserving Python/C/Rust controller
interfaces and keeping the command boundary explicit.

Rhai supports exposing native Rust functions and organizing them into modules:
[native functions](https://rhai.rs/book/rust/functions.html),
[modules](https://rhai.rs/book/rust/modules/index.html).

## Foundations (connector-level) — do these first

### 1. Rate-carrying connectors

**What.** Rotational and translational connectors gain explicit rate lanes:
`(angle, speed | torque, —)` and `(position, velocity | force, —)`. The
compiler adds the identity row `speed = d(angle)/dt` per node, so the rate
is a *node unknown* every element can read exactly at any instant — not the
step-average `across_rate` that made the sampled controller sample half a
step late.

**Library changes.** `ConnectorKind::lanes` for Rotational/Translational
(width 2, one through); `sim-compile` emits the derivative row for lanes
flagged `derivative_of: Some(lane)`; `Context::across_rate` becomes an alias
for the rate lane where one exists. Elements are unchanged except sensors,
which read the lane. Guards and jumps see rates for free.

**Example — the sampled loop, exactly.** Re-run plate 14 and tighten the
pole check from 2e-3 to 1e-6: the sampled speed is now the instantaneous
state, so the measured pole equals `e^(−T/τ) − KpK(1 − e^(−T/τ))` to the
integrator's own accuracy. What the lane changes is *instants*: on
continuous reads a step-average sensor coincides with it under the midpoint
rule (both are the mid-step speed), but a guard sees the lane at an
instant — a speed-trip latch on the shaft fires at `τ'·ln(ω₀/ω_trip)` to
1e-6, where before the lane guards could not see a rate at all.

### 2. Complementarity (unilateral) connectors

**What.** A `Contact` connector whose through lane is cone-constrained
(`N ≥ 0`, `|F_t| ≤ μN`) and whose across lane is half-space constrained
(`gap ≥ 0`, with `gap·N = 0`). Solved per step as a linear complementarity
problem inside the implicit step, with impacts as events carrying a
restitution law. This retires every penalty and every regularised
friction in the suite.

**Library changes.** `sim-dynamics`: a `Complementarity` set on `System`
(indices of paired lanes + cone parameters), a projected Gauss–Seidel /
Lemke solve inside `implicit_step` after Newton on the smooth part, and an
impact event that switches the constraint set. `sim-compile`: contact
connectors bind to frames or planar nodes and emit no balance row of their
own — their through is the multiplier. `sim-domain-multibody`:
`contact.point_plane`, `contact.sphere_plane`, `contact.point_line`.

**Example — Painlevé's paradox.** A rigid rod sliding on a rough plane,
leaning at angle θ, driven along the surface. For μ > μ_c(θ) the rigid-body
equations have *no* consistent solution with sliding contact and the rod
must jump (the paradox, Painlevé 1895; the critical value for a uniform rod
is μ_c = 4/(3 tan θ)·… as tabulated by Génot & Brogliato 1999). Knob: μ.
Change: smooth sliding → an impulsive "jam and hop". Number: the boundary
μ_c(θ) computed from the complementarity condition and hit by the run.
Falsifier: a compliant (penalty) contact never jumps — it just gets stiff.
Second example for free: the walker and the tippe top re-run with
`contact.*` in place of their event map and penalty, and their published
numbers must not move.

**Library note (implemented).** A velocity-level complementarity step can
have a solution that Newton cannot reach from the smooth predictor —
Painlevé's jam is exactly that: the sliding branch has no solution, the
stick branch does. Time-stepping codes enumerate contact branches by hand;
here a nonsmooth element proposes them: `Behavior::branches` returns
alternative starts (its own states plus across-lane overrides such as
"tangential velocity zero"), and `Simulation` tries each before subdividing
the step. Any future nonsmooth element (a ratchet, a diode, a clutch) gets
the same treatment for free.

### 3. Entropy as a thermal lane

**What.** Thermal becomes `(temperature | heat_flow, entropy_flow)`, and the
compiler asserts, at every step and every element, that the element's net
entropy production is non-negative. The second law becomes a *validated
property* of every model.

**Library changes.** Third lane on `ConnectorKind::Thermal`; every thermal
element reports `Ṡ_produced` through `Context::produce_entropy(value)`;
`sim-compile` sums it per element and the runtime fails a step on a
negative value beyond tolerance. Bridges (thermistor, thermoelastic layer,
motor copper loss) declare their dissipation once and get the entropy for
free.

**Example — the thermoelastic beam, closed.** Plate 16 currently computes
entropy production by hand in the scenario from layer temperatures. With
the lane, the check "T₀·∫Ṡ dt = mechanical energy lost" reads the
compiler's per-element production from the store and nothing else.
Falsifier: give one layer conductance a negative value — heat carried
uphill — and the runtime refuses the first step: a perpetual motion machine
of the second kind does not run. (A sign-reversed *reversible* bridge is not
caught this way, and should not be: reversibility is exactly what it
declares; its wrongness shows up as the beam gaining energy.)

### 4. Composite (mixed) bundles

**What.** A connector kind composed of other kinds: `Motor =
Electrical ⊕ Rotational ⊕ Thermal`, `Battery = Electrical ⊕ Thermal ⊕
Chemical`. A part exposes one port; the compiler splits it into its member
nodes.

**Library changes.** `ConnectorKind::Composite(&[ConnectorKind])`; port
declarations and connections are unchanged; `sim-compile` fans a composite
node out to member nodes and lanes offsets accordingly. Behaviors address
member lanes by `(port, member, lane)`.

**Example — a drive that knows its motor is hot.** Plate 3 (current
hogging) re-authored with two `Motor` composite ports on one drive: the
same hogging appears as two motors on a shared bus, one winding taking the
current as it warms. Knob: winding temperature coefficient. Number: the same
`|α|·R_th·P = 1` boundary. Falsifier: pin the two thermal members
together.

---

**Library note (implemented).** `ConnectorKind::Composite(&[…])` with
`ConnectorKind::MOTOR`; `ModelWorld::instantiate` fans a composite port out
into member ports (`plug.electrical`, `plug.rotational`, `plug.thermal`),
`connect` joins composites member-wise and lets plain ports join the member
of their kind, and the compiler binds a behavior's composite port as the
concatenation of its members (`ConnectorKind::member_offset`). Plate 18
(`motor-hogging`) is the example: `bridge.motor` and `bridge.dual_drive`.

## New physical domains

### 5. Magnetic

**Lanes.** `(mmf | flux_rate)` — a magnetic circuit is a network like an
electrical one; reluctance elements are resistors, coils are gyrator-like
bridges to electrical, and air gaps are reluctances that depend on a
mechanical across (a bridge to translational/rotational).

**Elements.** `magnetic.reluctance`, `magnetic.saturable_core` (B–H curve),
`magnetic.permanent_magnet`, `bridge.coil` (electrical ↔ magnetic),
`bridge.air_gap` (magnetic ↔ translational, force = ∂energy/∂gap),
`bridge.eddy_sheet` (magnetic ↔ thermal). Then `bridge.brushed_motor`
becomes a composition instead of a constant k_t.

**Example — the Levitron.** A spinning magnet above a base magnet is
unstable at rest (Earnshaw) and stable only inside a spin-rate window
(Berry 1996; Simon, Heflinger & Ridgway 1997: roughly 1 000–3 000 rpm for
the commercial toy). Knob: spin rate. Change: falls / flies / flips out.
Number: the two window edges from the linearised compiled model, hit by the
runs. Falsifier: remove the spin and the field gradient alone cannot hold
it — Earnshaw's theorem falls out of the same network.

**Library note (implemented).** `sim-domain-magnetic`: `ConnectorKind::Magnetic`
on `(mmf | flux_rate)`, `magnetic.ground/reluctance/saturable_core/
permanent_magnet`, `bridge.coil/air_gap/eddy_sheet` (crate tests: a coil on
a reluctance is `L = N²/R`, the gap pull is `Φ²/2μ₀A`, the loop field is
divergence-free), `LoopField` (exact K/E) and `bridge.magnetic_top`. Plate
19 (`levitron`). Two solver lessons came with it: Newton now accepts
stagnation at the residual's noise floor as convergence (it used to churn
to 40 iterations and subdivide), and `analysis::monodromy` takes its
perturbation scale explicitly (1e-3–1e-4; the flow's convergence noise is
~1e-9 and a 1e-6 difference quotient amplifies it into fake multipliers).

### 6. Two-phase fluid with an equation of state

**Lanes.** Hydraulic becomes a two-lane bundle: `(pressure, enthalpy |
mass_flow, enthalpy_flow)`. Volumes carry mass and energy; an equation of
state (`fluid.water_iapws_lite` — a compact saturation-curve fit is enough)
decides phase, density and temperature from (p, h).

**Elements.** `fluid.volume_ph`, `fluid.pipe_ph` (inertance + wall
friction), `fluid.valve_ph`, `fluid.pump`, `bridge.wall_heat` (fluid ↔
thermal), `fluid.eos_water`.

**Example — the geyser.** A vertical column heated at the bottom: pressure
from the water above suppresses boiling until a bubble forms, lifts the
column, drops the pressure on the fluid below, and the whole column flashes
(the geyser cycle; Ingebritsen & Rojstaczer 1993 give the period–heat-flux
relation). Knob: bottom heat flux. Change: steady simmer → periodic
eruptions with a period that *falls* as heat rises. Number: eruption period
vs heat flux slope from the reference. Falsifier: hold the column pressure
fixed (an open top of zero height) and there is no eruption, only boiling.

**Library note (implemented).** `ConnectorKind::FluidPh` and
`sim_domain_fluid::twophase` (`fluid.volume_ph/pipe_ph/valve_ph/pump/
reservoir_ph/tank_ph`, `bridge.wall_heat`, the `Water` EOS from (p, h);
volumes keep mass and energy as states and provide (p, h) through the EOS).
Plate 20 (`geyser`). Library changes it forced: each through lane balances
on its across unknown's row (an element may provide every lane of its
node); `Integrator::BackwardEuler`; `Behavior::pinned` for node
initialisation; three-stage consistent initialisation (rates → reactions →
authored values); Newton's stall acceptance on each unknown's own scale and
a bound on lost line searches.

### 7. Electrochemical

**Lanes.** `(chemical_potential | molar_flow)` per species, with a
temperature dependence that couples to thermal without special cases.

**Elements.** `chem.reservoir` (fixed activity), `chem.reaction`
(Arrhenius rate with activation energy; a bridge chemical ↔ thermal that
emits the enthalpy), `chem.diffusion`, `bridge.electrode` (electrical ↔
chemical, Butler–Volmer), `chem.cell` composite.

**Example — Semenov ignition.** A reacting mass in a vessel with a fixed
wall temperature: heat generation `A·e^(−E/RT)` against linear wall loss.
Below a critical wall temperature it settles; above it, it ignites. Knob:
wall temperature. Number: Semenov's criterion `ψ = (E/RT_w²)·(Q·V·A·e^(−E/RT_w))/(h·S)
= 1/e` (Semenov 1928; Frank-Kamenetskii's 1/e·… form) gives the wall
temperature, and the run's ignition boundary lands on it. Falsifier: set the
activation energy to zero (linear heat release) and there is no threshold at
all — the steady state moves smoothly.

**Library note (implemented).** `sim-domain-chemical` on
`ConnectorKind::Chemical` (`chem.reservoir/species/reaction/diffusion`,
`bridge.electrode`, `ConnectorKind::BATTERY`). Plate 21 (`semenov-ignition`).

### 8. Radiative transfer

**Lanes.** `(radiosity_potential | radiant_flux)` between surfaces, with
view factors as network elements; a bridge to thermal at each surface
enforces `q = εσ(T⁴ − …)`.

**Elements.** `radiation.surface` (emissivity, area, thermal port),
`radiation.view` (view factor between two surfaces), `radiation.sky`
(fixed effective temperature), `radiation.window` (spectral transmission as
two bands).

**Example — cooling below ambient.** A surface emissive in the atmospheric
window facing a clear sky, insulated from convection, settles *below* the
air temperature by day and further at night (Raman, Anoma, Zhu, Rephaeli &
Fan 2014 measured ≈ 5 K below ambient under direct sun). Knob: emissivity
in the 8–13 µm band. Change: warms with the day → cools below ambient.
Number: the sub-ambient depression from the energy balance with the
reference's sky model. Falsifier: a grey (single-band) emitter cannot do it;
the effect needs the band selectivity.

**Library note (implemented).** `sim-domain-radiative` on
`ConnectorKind::Radiative` (`radiation.surface/view/sky`, `Band`,
`planck_fraction`). Plate 22 (`sky-cooling`).

### 9. Distributed 1-D fields as elements

**What.** A `line` element (string, beam, pipe, transmission line, duct)
discretised at compile time with `cells` internal nodes, exposing ports at
*positions* along it (`tap = 0.25`). The Rijke duct and the water-hammer
pipe become instances instead of hand-assembled chains; anything can be
attached mid-line.

**Library changes.** A behavior may declare *parametric* ports
(`port("tap", position)`) so an instance can carry any number of taps; the
descriptor lists a port family, the instance names its members.

**Example — vortex-induced vibration lock-in.** A cable (line element,
Translational lanes along its length) in a cross-flow with a van der Pol
wake oscillator attached at every cell (`fluid.wake_oscillator`, Facchinetti,
de Langre & Biolley 2004). Sweep flow speed: the shedding frequency
`St·U/D` walks up, and across a band around each cable mode it *locks* to
the mode and the amplitude jumps. Knob: flow speed. Number: lock-in band
width and peak amplitude from the reference's tuned oscillator model.
Falsifier: fix the shedding frequency (remove the wake feedback) and there
is resonance but no lock-in — the plateau vanishes.

**Library note (implemented).** Port families: a descriptor port named
`prefix.*` gives an instance one port per `prefix.<name>` parameter, bound
after the fixed ports sorted by name. `sim-domain-line` (`line.string` with
`tap.*`), `fluid.wake_oscillator`. Plate 23 (`viv-lock-in`).

### 10. Granular

**Lanes.** `(pressure | particle_flow)` with a state-dependent
constitutive law: below a critical packing fraction the medium flows,
above it the network jams and the through lane becomes a constraint (this
reuses the complementarity machinery of item 2).

**Elements.** `granular.hopper`, `granular.column` (Janssen wall friction),
`granular.orifice` (Beverloo), `granular.jam` (a complementarity element
switching flow ↔ constraint).

**Example — Janssen's silo.** Fill a tall silo: the pressure at the base
rises with depth and then *saturates* at `ρgD/(4μK)` (Janssen 1895) —
adding grain adds no load. Knob: wall friction μ. Change: hydrostatic →
saturating. Number: the saturation pressure and the depth scale `D/(4μK)`.
Falsifier: μ = 0 gives hydrostatic pressure at every depth.

---

**Library note (implemented).** `sim-domain-granular` on
`ConnectorKind::Granular` (`granular.hopper/column/orifice/sink`; Janssen
inside the column, Beverloo with jamming in the orifice). Plate 24
(`janssen-silo`). The `granular.jam` complementarity element was not needed
for the example and is not there.

## Numerical facilities that are really connectors

### 11. Stochastic lanes

**What.** A through lane may carry a noise process (white or coloured,
with a declared spectral density): Johnson noise on a resistor, Langevin
force on a mass, shot noise on a diode. `sim-dynamics` integrates with a
stochastic implicit midpoint (drift implicit, diffusion explicit) and the
`Trace` gains an ensemble mode.

**Library changes.** `Context::add_noise(port, density)`; a seeded
generator in `Simulation`; `analysis::power_spectrum` and ensemble
statistics.

**Example — stochastic resonance.** A bistable element (double-well
translational spring) driven by a weak subthreshold periodic force plus
thermal noise: the output signal-to-noise ratio *peaks* at an intermediate
noise strength (Benzi, Sutera & Vulpiani 1981; Gammaitoni et al. 1998 give
the Kramers-rate optimum `2r_K = Ω`). Knob: noise intensity. Change: no
switching → synchronised switching → random switching. Number: the optimum
where the Kramers rate matches the drive frequency. Falsifier: remove the
second well and adding noise only ever hurts.

**Library note (implemented).** `Context::add_noise(port, intensity)`,
per-step draws held through the implicit solve (`System::begin_step`),
`Runtime::seed`, `analysis::{fft, power_spectrum}`;
`translational.double_well` and `translational.langevin`. Plate 25
(`stochastic-resonance`). No ensemble mode on `Trace` — a seed per run
does the job for the example.

### 12. Kinematic joint connectors between frames

**What.** A `Joint` connector between two owned frames (revolute,
prismatic, spherical, planar) that the compiler *eliminates* — the child
frame's states are expressed in the parent's, so a tree of bodies has
minimal coordinates and no multipliers. Closed loops keep a multiplier.

**Library changes.** `sim-compile` recognises joint connections and rewrites
the child frame's owned states as functions of joint coordinates (a
Featherstone-style tree pass); `sim-domain-multibody` gets
`joint.revolute`, `joint.prismatic`, `joint.spherical`, `joint.fixed`, and a
`body.link` with inertia about its own frame.

**Example — the quadruped, in the world.** Plate 32 assembles bodies,
chains, motors, sensing and contacts in a `ModelWorld`. The specialized
quadruped runtime and gait-synthesis backend have been removed. Future
joint elimination should be validated against analytic mechanisms and
these generic models, with topology changes reflected in their motion.

---

**Library note (implemented as an element; 3-D joints not).** `joint.revolute`, `joint.fixed` and `joint.prismatic` remain multiplier joints between owned frames (plate 26). The elimination pass exists as `multibody.chain` (plates 31 and 32): a planar serial chain in minimal coordinates — the links' poses are functions of the base frame and the joint angles, a recursive Newton–Euler pass with the joint accelerations as unknowns supplies the equations, each joint is a rotational port a motor plugs into, and the tip is an owned frame contacts attach to. It reproduces the double pendulum's notes to 0.1 %, conserves energy, and drags a free base along. The compiler itself does not eliminate joints between separately authored bodies, and spherical/universal 3-D joints are not done.

## Sequencing

| Step | Item | Unlocks |
|---|---|---|
| 1 | Rate lanes (1) | exact sensors/controllers; needed by 2 |
| 2 | Complementarity contacts (2) | honest walker, tippe top, Painlevé; needed by 10 |
| 3 | Entropy lane (3) | second-law validation for every later thermal bridge |
| 4 | Composite bundles (4) | motor/battery as single ports; needed by 5, 7 |
| 5 | Magnetic (5) | physically complete actuators, Levitron |
| 6 | Distributed lines (9) | Rijke and hammer as parts; VIV lock-in |
| 7 | Two-phase fluid (6) | geyser, boiling, cavitation |
| 8 | Electrochemical (7) | batteries, ignition |
| 9 | Radiative (8) | sky cooling, greenhouse |
| 10 | Granular (10) | Janssen, jamming |
| 11 | Stochastic lanes (11) | stochastic resonance, noise-limited sensors |
| 12 | Joints (12) | the quadruped in the model world |

Each step ends the same way: the example above is added to
`surprise-tests.md`, compiled through `sim-phenomena`, run against its
published number, and given an `Exhibit` so it is on the gallery and in the
viewer. A domain without a surprise on the gallery is not finished.
