# Surprise tests

The acceptance suite for the multi-domain core is a list of documented surprises:
behaviors nobody encoded that fall out of honest coupling. Every entry below has
the same shape.

- **One knob.** A single parameter at one boundary of the model.
- **A qualitative change.** Not "the number is a bit different" — the system does
  something categorically different on the other side of the knob.
- **A published number.** A closed-form threshold, a measured constant, or a
  reference trace from the literature that the run must hit within tolerance.
- **What it proves.** The specific piece of the architecture that would have to
  be wrong for the effect to disappear.
- **The falsifier.** The cheapest way to make the effect vanish. If the sim still
  shows the effect after the falsifier is applied, the test is not testing what
  we think it is.

Each scenario ships with a stored reference trace, so a regression is a number
that changed rather than a simulation that looks a bit wrong. All of them run on
the same three pieces of infrastructure: scenario runner, port-trace recorder,
comparison report.

Domain tags use the vocabulary from `index.html` plus `control`, `contact` and
`multibody`, which have crates of their own.

## Status

All sixteen are implemented in `crates/sim-phenomena` as **models of the
actual system**: each scenario authors a `ModelWorld` of registered behaviors
with typed ports and connectors (`sim-core`), `sim-compile` turns it into one
integrable island per connected component with a balance row per node
through-variable, `sim-dynamics` integrates it (implicit midpoint on the
DAE, with behavior-declared guards and jumps for events), and every number a
scenario reads comes back through `StateStore` ids. No scenario carries its
own residual, unknown layout or equations of motion; the physics lives in
the domain crates — rotational, translational, electrical, thermal,
hydraulic, acoustic, fluid, control, bridges and a rigid-body/planar
multibody domain — as reusable elements.

The reference calculations that sit on top of a model take the *compiled*
system as input: flutter and tippe-top stability boundaries are eigenvalues
of the compiled island's linearisation, the backlash describing function
reads the loop's transfer function from the same linearisation, the
sample-rate plant constants are measured on the compiled plant, and the
walker's stride map is the compiled walker run to its heel-strike event.

Every compiled scenario was diffed against the earlier hand-written
implementation before that implementation was removed: trace-level
agreement (1e-6 to 1e-8 on the state) for the smooth cases, event times to
1e-9 and second-order mutual convergence for the escapement clocks, and
limit-cycle amplitude and period to 2% for stick–slip past its first stick.

```sh
cargo run --release -p sim-phenomena -- all
cargo run --release -p sim-phenomena -- all --output runs/phenomena.json --html surprise-gallery.html
cargo run --release -p sim-app -- --scene phenomena
cargo test --release -p sim-phenomena
```

Where a run taught us something the original text did not know, the entry
below says so in an *Implementation note*.

---

## The original six

### 1. Kapitza's inverted pendulum
`mechanical`

Shake the pivot up and down fast enough and the *upright* position becomes
stable. Nudge the bob and it returns to vertical, standing on nothing.

- **Knob:** pivot drive frequency Ω (or amplitude a).
- **Change:** the inverted equilibrium switches from unstable to stable.
- **Number:** stability requires a²Ω² > 2gL. Small oscillations about the
  inverted point occur at ω² = a²Ω²/(2L²) − g/L (Kapitza 1951; Landau &
  Lifshitz *Mechanics* §30).
- **Proves:** integrator fidelity and timescale separation. The effect lives
  entirely in the averaged residual of a fast oscillation, so a step size that's
  merely adequate erases it completely.
- **Falsifier:** drop the drive frequency below the threshold; the bob must fall.

### 2. Huygens' coupled clocks
`mechanical` `structural`

Two pendulum clocks on a shared flexible beam settle into exact anti-phase
within the hour, whatever their starting positions. The beam moves by
micrometers.

- **Knob:** beam stiffness (equivalently, beam mass relative to the clocks).
- **Change:** from independent drift to phase-locked anti-phase.
- **Number:** anti-phase locking as in Bennett, Schatz, Rockwood & Wiesenfeld
  2002 (*Proc. R. Soc. A* 458); locking time and the beam-mass ratio window
  from their model.
- **Proves:** weak coupling survives the connector layer. All the information
  passes through a tiny amount of momentum exchange — round it away and the
  synchronization never appears.
- **Falsifier:** make the beam rigid; the clocks must drift independently.

*Implementation note.* Escapement modelled as a fixed angular-impulse kick at
each zero crossing; beam 5 kg at 1 Hz, ζ = 0.5, pendulums 0.5 kg on 0.994 m.
Both a near-in-phase and a quadrature start lock to |Δφ| = 3.137 rad within
40 minutes with the beam moving 37 µm. A 20 kg beam locks *in-phase* from the
near-in-phase start, as Czolczynski et al. 2011 report, so the beam mass is a
real knob too.

### 3. Current hogging
`electrical` `thermal`

Two identical devices in parallel should split the load evenly. With a
positive temperature coefficient they self-balance; flip that coefficient
negative and the marginally hotter one draws more current, heats further, and
takes all of it.

- **Knob:** the sign of dR/dT.
- **Change:** from a stable even split to one device carrying the whole load.
- **Number:** the equal split is stable iff the loop gain
  (∂P/∂T · R_th · |dR/dT| / R) is below 1 for the negative-coefficient case;
  the run must cross that boundary where the formula says it does.
- **Proves:** the electro-thermal loop closes in both directions — dissipation
  heats the part, temperature moves resistance — with no special case anywhere.
- **Falsifier:** set the thermal resistance between the devices to zero; they
  share a temperature and must split evenly regardless of coefficient sign.

*Implementation note.* With `R = R₀·exp(α·ΔT)` the loop gain is
`G = |α|·R_th·P` at the symmetric operating point and the asymmetry grows at
`(G − 1)/(R_th·C)` exactly — measured to 1e-5 either side of G = 1. A positive
coefficient on a *current* source is only common-mode stable while
`α·R_th·P < 1`; the PTC case runs at G = 0.8 for that reason.

### 4. The Rijke tube
`thermal` `acoustic`

A heated gauze a quarter of the way up an open tube makes it sing. Steady heat
in, a loud pure tone out, with no oscillator anywhere in the model. Slide the
gauze past the midpoint and it falls silent.

- **Knob:** gauze position along the tube.
- **Change:** silent → sustained tone at the tube's fundamental → silent.
- **Number:** oscillation only for gauze in the lower half, strongest near
  L/4; tone frequency = c/2L (Rayleigh 1878; Heckl 1990).
- **Proves:** Rayleigh's criterion emerges rather than being asserted — heat
  added in phase with the pressure wave feeds the mode. The position dependence
  makes it sharply falsifiable.
- **Falsifier:** move the gauze to 3L/4; the tube must be silent.

*Implementation note.* Three Galerkin modes, King's-law heat release through
an eight-stage Erlang lag (τ = 0.2), damping ζ_j = 0.1j² + 0.06√j, β = 1.
Growth rates at x_f = 0.10, 0.15, 0.25, 0.35, 0.45 are −0.09, 0.006, 0.19,
0.16, 0.0001: strongest at L/4. The tone sits 3% above c/2L — the heater
shifts the frequency, as it does in real tubes. Between x_f = 0.5 and 0.75 the
*second* harmonic can be driven; at 3L/4 the tube is silent.

### 5. Water hammer
`hydraulic` `structural`

Close a valve in a tenth of a second and pressure spikes to many times what
the pump can deliver — routinely enough to split the pipe.

- **Knob:** valve closure time relative to 2L/c.
- **Change:** a gentle pressure rise becomes a full Joukowsky spike.
- **Number:** Δp = ρ · c · Δv for closure faster than 2L/c (Joukowsky 1898);
  wave speed c from fluid bulk modulus and pipe-wall compliance.
- **Proves:** we're carrying compressibility and a finite wave speed. An
  incompressible network solve cannot produce this transient at all.
- **Falsifier:** close the valve over 10 × 2L/c; the spike must collapse to the
  quasi-static value.

*Implementation note.* 40-cell staggered grid, L = 100 m, c = 1200 m/s,
v₀ = 2 m/s, so ρcΔv = 2.4 MPa on a 1 bar head. The plateau after a 20 ms
closure matches Joukowsky within 3% and the ringing period is 4L/c within 3%.
Closing over ten round trips leaves a peak of 0.26 ρcΔv (a conductance-linear
valve, not the velocity-linear one Michaud's 0.1 assumes).

### 6. Flutter
`fluid` `structural`

Below a critical airspeed the structure damps any disturbance. A few metres
per second above it, the same structure starts extracting energy from the
airflow and shakes itself apart. Nothing changed but one boundary value.

- **Knob:** freestream velocity.
- **Change:** decaying response → growing response.
- **Number:** the 2-DOF pitch–plunge section flutter speed from Theodorsen
  1935 (or the quasi-steady approximation in Bisplinghoff, Ashley & Halfman),
  for a published parameter set.
- **Proves:** the system finds a stability boundary we never wrote down — the
  phase lag that inverts the sign of the damping is a consequence of the
  coupling, not a term in it.
- **Falsifier:** lock pitch and plunge together (single DOF); flutter must not
  occur at any speed.

*Implementation note.* Quasi-steady lift `2πρU²b(α + ḣ/U + b(½−a)α̇/U)`
about an axis at a = −0.2, x_α = 0.15, r_α = 0.55, 4 Hz plunge, 10 Hz pitch.
The eigenvalue boundary is 15.619 m/s at 64.6 rad/s; the time-domain
bisection gives 15.618 m/s. Theodorsen's unsteady lift is the next fidelity
step; the quasi-steady boundary is the reference here.

---

## Ten more

### 7. The Dzhanibekov flip
`multibody`

Spin a rigid body about its intermediate principal axis and it periodically
flips end over end, apparently spontaneously. Spin it about the longest or
shortest axis and it just spins.

- **Knob:** which principal axis carries the initial angular velocity.
- **Change:** steady spin (axes 1 and 3) vs. periodic 180° tumbles (axis 2).
- **Number:** perturbation growth rate λ = ω₂ √((I₂−I₁)(I₃−I₂)/(I₁I₃)); the
  full motion is the Jacobi-elliptic solution of Euler's equations (Landau &
  Lifshitz §37; Ashbaugh, Chicone & Cushman 1991, *J. Dyn. Diff. Eq.*). Flip
  interval scales as (2/λ)·ln(1/ε) with initial perturbation ε.
- **Proves:** the 6-DOF rotational core integrates attitude without drift.
  Kinetic energy and |L| must both be conserved to tolerance *through* a flip;
  a non-symplectic or badly normalized quaternion integrator will either damp
  the flip out or spin up.
- **Falsifier:** make the body axisymmetric (I₁ = I₂); no flip at any spin rate.

*Implementation note.* Implicit midpoint conserves the quadratic invariants
(kinetic energy, |L|², |q|²) to round-off through every flip; the world-frame
angular momentum — not quadratic — drifts by < 1e-6 over 120 s. The flip
interval matches the Jacobi-elliptic period, 55.062 s, to 1e-8.

### 8. The tippe top
`multibody` `contact`

Spin a squat top with a stem and it turns itself upside down and keeps
spinning on the stem, raising its own center of mass. Sliding friction at the
contact point does the work.

- **Knob:** initial spin rate (and, for the falsifier, friction coefficient μ).
- **Change:** wobbles and settles vs. fully inverts.
- **Number:** inversion above a critical spin rate given by the linearized
  stability of the non-inverted state (Bou-Rabee, Marsden & Romero 2004, *SIAM
  J. Appl. Dyn. Sys.*; Cohen 1977 *Am. J. Phys.*). Total energy must decrease
  monotonically throughout — friction is the only dissipation and it can't run
  backwards.
- **Proves:** rolling/sliding contact with friction is doing honest work: it
  moves energy from spin into potential energy while dissipating the rest, and
  the gyroscopic coupling that steers the process is emergent.
- **Falsifier:** μ = 0. The top must never invert, no matter how fast it spins.

*Implementation note.* This is the standard mathematical tippe top of the
cited papers: a sphere (R = 15 mm, a = 3 mm) with the centre of mass on the
axis and *no* stem contact; inversion is the axis swinging to θ = π with the
COM rising by 2a. Modelling a hard stem tip stalls the top at the two-contact
angle, because with the COM outside the support segment nothing static can
lift the sphere. Friction is Coulomb (μ = 0.3) with a 5 mm/s regularisation,
so the upright state linearises; its critical spin is 32.4 rad/s, and at
6 ω_c the top inverts in about six seconds with energy monotone to 1e-9.

### 9. Passive dynamic walker
`multibody` `contact`

A two-legged linkage with no motors and no controller, placed on a shallow
slope, walks. Steepen the slope and its gait period-doubles, then goes
chaotic, then it falls.

- **Knob:** slope angle γ.
- **Change:** falls → period-1 gait → period-2 → period-4 → chaotic gait →
  falls.
- **Number:** for the "simplest walking model" (Garcia, Chatterjee, Ruina &
  Coleman 1998, *J. Biomech. Eng.*), the γ = 0.009 fixed point θ* = 0.200310;
  stable period-1 gait for γ below ≈ 0.015 rad, period doubling at ≈ 0.0151,
  period-4 near 0.0177, aperiodic by 0.019.
- **Proves:** hybrid dynamics with impulsive contact events are exact enough
  that a bifurcation cascade lands at the published slopes. Also the energy
  ledger: at steady gait, gravity's work per step equals heel-strike loss per
  step. This is the standing test for the quadruped and leg work.
- **Falsifier:** make heel-strike perfectly elastic; no steady gait can exist.

*Implementation note.* The stride map is analysed as the paper does it —
Newton for fixed points, Floquet multipliers for stability. At γ = 0.009 the
fixed point is θ* = 0.200311 (paper: 0.200310); the period-1 multiplier
crosses −1 at γ = 0.015087 and the period-2 orbit's at 0.017671. Past that
the stride sequence is aperiodic. In this implementation the walker does
*not* fall beyond γ ≈ 0.019 — the chaotic gait persists to much steeper
slopes with a widening stride-angle spread — so that claim is dropped from
the checks.

### 10. Euler buckling
`structural`

Push on the end of a slender column. Below a critical load it shortens
slightly and stays straight. Above it, it bows sideways — direction chosen by
whatever perturbation is present.

- **Knob:** axial compressive load P.
- **Change:** straight → bowed; symmetry breaks.
- **Number:** P_cr = π²EI/(KL)², K = 1 pinned–pinned. The continuous check:
  the lowest lateral natural frequency obeys ω² ∝ (1 − P/P_cr) and goes to zero
  at the boundary, so a frequency sweep pins P_cr without ever reaching it.
- **Proves:** geometric nonlinearity in the structural domain — the stiffness
  matrix must depend on load. A linear beam element never buckles.
- **Falsifier:** switch the element to small-strain linear; the column must
  stay straight at 2·P_cr.

*Implementation note.* A 12-link Hencky chain (EI = 1, L = 1). Its exact
discrete critical load is 4EI N²/L²·sin²(π/2N) = 9.813; ω² → 0 extrapolated
from four sub-critical loads gives 9.65 (1.7% low, 2.2% below π²).

### 11. The 2:1 spring pendulum
`mechanical`

A mass on a spring, allowed to swing as well as bounce. Tune the spring so its
bounce frequency is exactly twice the swing frequency, start it bouncing
straight up and down, and it spontaneously starts swinging — then hands the
energy back and bounces again, forever.

- **Knob:** spring stiffness k, tuned to k/m = 4g/L (bounce = 2 × swing).
- **Change:** pure bounce stays pure bounce (detuned) vs. periodic full
  exchange between bounce and swing (tuned).
- **Number:** autoparametric resonance of Vitt & Gorelik 1933; exchange
  period and its scaling with amplitude from Lynch 2002 (*Int. J. Non-Linear
  Mech.* 37). Detune by 10% and the transferred fraction collapses.
- **Proves:** nonlinear mode coupling comes out of the plain equations of a
  single mechanical island — no coupling term was written. The sharp tuning
  dependence catches any integrator that shifts frequencies.
- **Falsifier:** detune to k/m = 3g/L; the swing amplitude must stay at the
  perturbation level.

### 12. Stick–slip self-excitation
`mechanical` `contact`

A block on a moving belt, held by a spring, sits still at first — then starts
jerking back and forth with no forcing anywhere. Speed the belt up past a
threshold and the jerking stops. Bowed strings, brake squeal and chalk on a
board are the same mechanism.

- **Knob:** belt speed v_b (with a Stribeck-type velocity-weakening friction
  curve).
- **Change:** stable rest in the sliding equilibrium vs. a stick–slip limit
  cycle.
- **Number:** the equilibrium is unstable iff −dF/dv at v_b exceeds the
  viscous damping c (Popp & Stelter 1990). Above the belt speed where the
  friction curve flattens, oscillation must die. Extension: on a string, the
  bow-point velocity is a two-level square wave and the displacement a
  sawtooth — Helmholtz motion (Helmholtz 1862; McIntyre, Schumacher &
  Woodhouse 1983).
- **Proves:** the friction model and its stick/slip event detection produce
  self-oscillation from a genuinely non-smooth constitutive law, with the
  threshold in the right place.
- **Falsifier:** replace the friction curve with constant Coulomb friction (no
  velocity weakening); the block must settle.

### 13. Backlash hunting
`control` `rotational`

A servo with integral action driving a load through a gearbox with a little
play. It never quite settles: it hunts back and forth at a fixed frequency
whose amplitude is set by how much play there is. Tighten the gears and the
hunting shrinks in proportion; remove the play and it stops.

- **Knob:** backlash width b in the gear mesh.
- **Change:** clean settling (b = 0) vs. sustained limit cycle (b > 0).
- **Number:** describing-function prediction (Gelb & Vander Velde 1968):
  limit-cycle amplitude scales linearly with b, frequency sits at the phase
  crossover of the linear loop and is independent of b.
- **Proves:** the first vertical slice — sampled controller, motor, gears —
  composes causal and acausal parts honestly enough that a non-smooth
  mechanical element reaches back into the control loop. Halve b, amplitude
  halves, frequency unchanged.
- **Falsifier:** b = 0. The loop must settle with no oscillation.

*Implementation note.* The gap is a compliant mesh with a dead zone, in a PI
position loop that is conditionally stable: full mesh stiffness stable, a
tenth of it unstable. Doubling the gap doubles the amplitude (×1.99997) at
the same frequency (×1.000005) — exact by scale invariance — and the
describing function predicts the frequency within 3% and amplitude within 16%.

### 14. Sample-rate instability
`control` `electrical` `rotational`

A digital P controller that works fine at one sample rate. Slow the sampling
down and, at an exact sample period, the closed loop goes unstable — even
though the continuous version of the same loop is stable at any gain.

- **Knob:** controller sample period T.
- **Change:** converges to setpoint vs. diverges with growing oscillation.
- **Number:** for a first-order plant K/(τs+1) under zero-order hold and
  proportional gain K_p, the closed-loop pole is
  z = e^(−T/τ) − K_p·K·(1 − e^(−T/τ)), stable iff K_p·K < coth(T/2τ). The run
  must go unstable within tolerance of that T. Same test with a transport
  delay in place of the sampler.
- **Proves:** the sampled controller is genuinely hybrid with the continuous
  plant — the hold, the delay and the sample instants are all real, not
  approximated away by a small-step continuous controller.
- **Falsifier:** T → 0 (or a continuous PI). No instability at any gain.

### 15. Chua's circuit
`electrical`

Five components — two capacitors, an inductor, a resistor and one piecewise-
linear negative-resistance element — and it produces a double-scroll strange
attractor. Sweep the resistor and you walk through a period-doubling cascade
on the way in.

- **Knob:** the linear resistor R (dimensionless α).
- **Change:** fixed point → period-1 → period-2 → period-4 → chaos.
- **Number:** the canonical set α = 15.6, β = 28, m₀ = −8/7, m₁ = −5/7 (Chua,
  Komuro & Matsumoto 1986, *IEEE Trans. CAS*); successive bifurcation
  intervals ratio approaching Feigenbaum's δ = 4.669…; largest Lyapunov
  exponent positive in the chaotic regime.
- **Proves:** the DAE solver handles a piecewise-linear nonlinearity without
  numerically damping a strange attractor into a limit cycle. It is also the
  standing test of the determinism guarantee: two runs from the same seed must
  be bitwise identical after thousands of cycles of a chaotic system — the
  first case where "close enough" is not reproducible.
- **Falsifier:** set m₀ = m₁ > 0 (an ordinary resistor); the circuit must decay
  to rest.

*Implementation note.* The cascade is swept at Matsumoto's β = 100/7 from
α = 8.10 to 8.48 (period 2 near 8.17, 4 near 8.40, 8 near 8.44, then chaos);
the double scroll and Lyapunov exponent (0.46) use α = 15.6, β = 28. With
three bifurcations the ratio estimate is loose, so the check is ±45% of δ.

### 16. Thermoelastic damping
`structural` `thermal`

A vibrating beam with no damper anywhere in the model gradually stops. Bending
compresses one face and stretches the other; the compressed side warms, the
stretched side cools, heat flows across, and that flow is irreversible. The
damping is sharpest when the thermal crossing time matches the vibration
period.

- **Knob:** beam thickness h (which sets thermal relaxation time
  τ = h²/(π²χ), χ = thermal diffusivity).
- **Change:** nearly undamped (very thick or very thin) vs. peak damping at
  ωτ = 1.
- **Number:** Zener 1937/1938: Q⁻¹ = (Eα²T₀/ρc_p) · ωτ/(1+ω²τ²), with the
  peak location and height confirmed in Lifshitz & Roukes 2000 (*Phys. Rev.
  B* 61). Entropy production integrated over a cycle must equal the mechanical
  energy lost divided by T₀.
- **Proves:** the thermo-mechanical bridge works in both directions —
  strain-rate produces heat, temperature gradient produces stress — and the
  thermal port's entropy bookkeeping is honest. Dissipation appears where no
  dissipation was modelled.
- **Falsifier:** set the thermal expansion coefficient α = 0; the beam must
  ring indefinitely.

*Implementation note.* One flexural mode of a 1 mm × 124 µm aluminium beam at
10 kHz, sixteen thermal layers. Measured Q⁻¹ at h/3, h/√3, h, √3h, 3h all fall
within 5% of Lifshitz–Roukes; the peak is 0.494 Δ_E at ωτ = 1; and T₀ times
the integrated entropy production equals the energy lost to 1e-6 when both
are evaluated at the integrator's midpoint states.

---

### 17. Painlevé's paradox
`multibody` `contact`

A stick pushed tip-first across a rough floor slides. Raise the friction
past a bound that depends only on the stick's angle, and the rigid-body
equations have *no* sliding solution at all: the tip jams, with a normal
force that is impulsive — an impact without collision.

- **Knob:** friction coefficient μ.
- **Change:** smooth sliding with a finite normal force vs. an instantaneous
  jam.
- **Number:** Painlevé 1895; Génot & Brogliato 1999: for a uniform rod at
  angle θ from the plane, no consistent sliding solution when
  μ > μ_c(θ) = (1 + 3cos²θ)/(3 sinθ cosθ) — μ_c(60°) = 1.347. Below it the
  sliding normal force is n = mg/(1 + 3cos²θ − 3μ sinθ cosθ), diverging at
  μ_c.
- **Proves:** the contact domain is a genuine complementarity (0 ≤ n ⊥ gap
  ≥ 0, Coulomb cone), not a penalty in disguise: a penalty always has a
  solution, and cannot produce the jam. It also proves the step solver can
  reach a branch the smooth predictor cannot see.
- **Falsifier:** replace the rigid contact by a compliant one; it stiffens
  and never jams.

*Implementation note.* A planar rigid body (`planar.rigid_body`) with a
`contact.point_plane` at its tip, started at 60° with the tip moving at 3 m/s.
At 0.5, 0.7 and 0.9 μ_c the measured normal force matches the closed-form
sliding solution to 1e-5 (1.14, 1.90 and 5.71 weights); at 1.3 μ_c the tip's
3 m/s is removed within a single 0.1 ms step and the bisected boundary lands
within 8% of μ_c. The jam is found through `Behavior::branches`: when the
smooth predictor fails, the contact proposes its stick branch and Newton
finishes it — the library's form of the branch enumeration nonsmooth
time-stepping schemes do by hand. A branch step is always backward Euler:
under the midpoint rule the impulse zeroes the tip velocity at the half
step and the end-of-step velocity is the start reflected, an elastic
bounce the physics does not contain.

---

### 18. Motor hogging on composite plugs
`composite` `electrical` `rotational` `thermal`

Plate 3 again, but nobody wires a winding, a shaft and a case separately:
each motor sits behind one `Motor` plug, and one drive holds two of them.
The drive regulates the total current, lets the windings share it on one
internal bus, and reads the hotter case off the plug it already holds.

- **Knob:** winding temperature coefficient α.
- **Change:** even split vs. the warmer motor taking the whole current.
- **Number:** the same `|α|·R_th·P = 1` boundary as plate 3, with the
  differential-mode growth rate `((|α|R_th P − 1)/R_th)/C`. With the rotors
  free, back-EMF adds `k²/c` of temperature-independent resistance per
  winding, so the loop gain scales by `R/(R + k²/c)`.
- **Proves:** `ConnectorKind::Composite` fans one port out into member
  nodes the compiler treats like any other; plain ports join the member of
  their kind; a behavior addresses `(port, member, lane)`; entropy accounting
  finds the thermal member inside the bundle.
- **Falsifier:** pin the two cases together (a large conductance between
  the thermal members) and the split stays even.

*Implementation note.* `bridge.motor` (one `Motor` plug, thermistor-law
winding) and `bridge.dual_drive` (two `Motor` sockets, a `hottest` signal).
Locked rotors: the hot motor takes 0.87 of the current at gain 1.5, the
compiled linearisation and the run both give ±0.0100 /s at gains 0.9 and
1.1. Free rotors: a light bearing (k²/c = 0.025 Ω, gain 1.46) still hogs, a
heavy one (k²/c = 2.5 Ω, gain 0.43) splits evenly — the shifted boundary,
not a numerical accident. Finding the consistent initial state of this
model needed `make_consistent` to solve in the minimum-norm sense, because
a clamped shaft's `θ = 0` row holds only a differential value.

---

### 19. The Levitron
`magnetic` `multibody`

A spinning magnetic top floats above a ring magnet with nothing touching it.
Stop the spin and it falls: no arrangement of static magnets can hold a
magnet in stable equilibrium (Earnshaw 1842). Spin it too fast and it also
falls — or slides out sideways.

- **Knob:** spin rate.
- **Change:** falls / flies / flips out.
- **Number:** Berry 1996 (*Proc. R. Soc. A* 452); Simon, Heflinger &
  Ridgway 1997 (*Am. J. Phys.* 65): the adiabatic trap `μ|B| + mgz` is
  stable only for `B'' > 0` and `B'' < B'²/(2B)` — for a loop field,
  heights between `a/2` and `a√0.4` — and the spin window (roughly
  1 000–3 000 rpm for the toy) has both edges given by the linear stability
  of the full rigid-body dynamics.
- **Proves:** the magnetic domain's field bridge drives a 6-DOF body
  (force `∇(μ·B)`, torque `μ×B`, energy `−μ·B`), and the analysis layer
  (`linear::linearise` on the compiled DAE) predicts both window edges,
  which the nonlinear runs then hit.
- **Falsifier:** remove the spin; the same network falls at once — Earnshaw.

*Implementation note.* `bridge.magnetic_top` (a dipole on a `Frame` port in
the exact elliptic-integral field of a loop, `sim_domain_magnetic::LoopField`)
on `multibody.rigid_body`. Ring radius 5 cm, 30 mT at the centre, top 20 g,
I_axial 2.2 µg·m², levitating 28 mm up (trap window 25.0–31.6 mm), dipole
0.385 A·m² sized to hold it there. A spinning top is a periodic orbit, not
a fixed point — and one whose quaternion returns as −q after a turn, so its
period is two turns — so the window comes from Floquet multipliers
(`analysis::floquet_multipliers` on the compiled runtime's flow, with the
spin-phase and quaternion-norm directions projected out): 863–1 869 rpm.
No spin: falls at 0.12 s. 10 % below the lower edge: 0.22 s; 10 % above
it, mid-window, and 10 % below the upper edge: aloft for the full 8 s;
15 % above the upper edge: out at 0.8 s. The magnetic circuit elements that arrived with the
domain — `magnetic.reluctance/saturable_core/permanent_magnet`,
`bridge.coil/air_gap/eddy_sheet` — are checked in the crate's own tests (a
coil on a reluctance is `L = N²/R`; the air-gap pull is `Φ²/2μ₀A`).

---

### 20. The geyser
`fluid` (two-phase) `thermal`

A column of water heated at the bottom and fed from an aquifer. The weight
of the water above holds the bottom well past the surface boiling point;
when it finally boils, the first steam lightens the column, the pressure on
the fluid below drops, and the whole column flashes and erupts. Cold water
refills it and the clock restarts — faster the harder it is heated.

- **Knob:** heat into the bottom.
- **Change:** a filling column that simmers vs. eruptions over the rim,
  sooner the harder it is heated.
- **Number:** Ingebritsen & Rojstaczer 1993 (*Science* 262): the clock is
  the energy needed to bring the water at the bottom to boiling — time to
  eruption ∝ 1/(heat − recharge cooling), a log–log slope of −1; and the
  bottom boils at the pressurised boiling point (120 °C under 11 m of
  water) rather than at 100 °C.
- **Proves:** a two-lane fluid connector `(pressure, enthalpy | mass_flow,
  enthalpy_flow)` with volumes that carry mass and energy and *provide*
  both across lanes, pipes whose weight follows the mean density of what
  they hold, upwind enthalpy transport, an equation of state that decides
  phase from (p, h), and a thermal bridge with honest entropy.
- **Falsifier:** lay the column flat — no water above the bottom — and the
  bottom can never store superheat: it boils at 100 °C the moment it gets
  there.

*Implementation note.* `fluid.volume_ph/pipe_ph/valve_ph/reservoir_ph`,
`fluid.tank_ph` (an open basin: pressure from its own head, vapour
separating at the surface, spill over the rim), `bridge.wall_heat`,
`fluid::twophase::Water` (Clausius–Clapeyron saturation, ideal-gas steam,
slightly compressible liquid, phase edges smoothed over 1 % of the latent
heat). Five 2 m segments of a 0.3 m conduit under a 1 m basin, an aquifer
0.5 m above the rim through an orifice. Volumes carry mass and energy as
their states and provide (p, h) through the equation of state — the
formulation that walks through a flash. At 100 kW the bottom stays liquid
to 118.7 °C; the eruption begins at the *top*, where the hot water pushed
up by expansion meets lower pressure and flashes first — the real cascade —
and works down; a burst of eruptions follows, after
which this homogeneous column settles into a perpetual spouter (steady
boiling, ~0.24 kg/s over the rim) — itself a real geothermal regime, and
the honest limit of a mixture model with no separate steam pockets. The
column's own acoustics (an 8 ms mode) are not the story, so this plate runs
on `Integrator::BackwardEuler`, which damps them. Library lessons: a
through lane's balance row is its across unknown's row whoever owns it;
consistent initialisation adjusts rates, then reaction-like unknowns, and
only then authored values; pinning elements (`Behavior::pinned`) start
their nodes at the pinned value; Newton's stall-at-the-floor acceptance is
judged on each unknown's own scale.

---

### 21. Semenov ignition
`chemical` `thermal`

A reacting mixture in a vessel whose wall is held at a fixed temperature.
Warm the wall a little and the vessel simmers a few kelvin above it; warm it
a little more and it explodes. There is a sharp threshold, and nothing in
the chemistry is discontinuous.

- **Knob:** wall temperature.
- **Change:** settles vs. ignites.
- **Number:** Semenov 1928 (Frank-Kamenetskii's form): criticality at
  `ψ = (E/RT_w²)·(Q·V·A·c·e^{−E/RT_w})/(hS) = 1/e`; exactly, where the
  Arrhenius generation curve is tangent to the linear loss line.
- **Proves:** a chemical connector `(chemical_potential | molar_flow)`
  whose concentrations follow the temperature they read off an ordinary
  thermal port, an Arrhenius reaction that hands its enthalpy to the thermal
  network with honest entropy, and an analysis layer that finds the
  threshold both ways.
- **Falsifier:** set the activation energy to zero — linear heat release —
  and there is no threshold: the steady state moves smoothly with the wall.

*Implementation note.* `chem.reservoir` (fixed concentration, so the fuel is
not consumed — Semenov's assumption), `chem.reaction`, a thermal capacitance
and a wall conductance to an ambient. E = 100 kJ/mol, A = 10¹⁰ s⁻¹,
c = 100 mol/m³, Q = 200 kJ/mol, V = 1 L, hS = 1 W/K. The runs' bisected
boundary is 383.26 K; the exact tangency gives 383.10 K (0.04 %) and
Semenov's ψ = 1/e gives 382.67 K (0.15 %, the size of RT_w/E = 0.032). With
E = 0 the steady excess is 4.64 K at every wall temperature.

---

### 22. Cooling below ambient under the sun
`radiative` `thermal`

A panel on a roof in direct sunlight ends up colder than the air around
it. It emits strongly in the 8–13 µm band, where the atmosphere is
transparent and the panel sees the cold of space; it reflects nearly all
the sunlight; and outside that band it barely emits, so the warm atmosphere
cannot heat it back.

- **Knob:** solar absorptivity (a grey emitter's is ~0.9).
- **Change:** cools below the air vs. bakes above it.
- **Number:** Raman, Anoma, Zhu, Rephaeli & Fan 2014 (*Nature* 515):
  ≈ 4.9 K below ambient under ~890 W/m² of sun, with ~3 % solar absorption
  and a nonradiative coefficient ≈ 6.9 W/m²K.
- **Proves:** a radiative connector `(radiosity | radiant_flux)` with
  band-limited surfaces (Planck band fractions), view factors and a sky per
  band, exchanging with the thermal network through ordinary ports.
- **Falsifier:** a grey emitter is black to the sun as well: it absorbs
  ~800 W/m² and heats far above ambient.

*Implementation note.* Three bands (below 8 µm, the 8–13 µm window, above
13 µm), each a `radiation.surface` – `radiation.view` – `radiation.sky`
chain on one thermal node; the window's sky is space through a dry
atmosphere (255 K effective), the other bands see the air. Selective
emitter (ε = 0.9 in the window, 0.1 elsewhere, α_solar = 0.03): 5.17 K below
the air in the sun, 8.06 K at night; the compiled network reproduces the
hand energy balance to 10⁻⁴ K. Grey (0.9 everywhere): 51.8 K above the
air.

---

### 23. Vortex-induced vibration lock-in
`line` `fluid`

A taut cable in a cross-flow sheds vortices at the Strouhal frequency
`St·U/D`, which rises with the flow speed. Sweep the speed past a cable
mode and you do not get a resonance peak: across a whole band of speeds
the shedding *locks* to the mode, the cable rings at its own frequency
though the wake "should" have moved on, and the amplitude plateaus.

- **Knob:** flow speed.
- **Change:** a narrow resonance vs. a broad lock-in plateau.
- **Number:** Facchinetti, de Langre & Biolley 2004 (*J. Fluids Struct.*
  19): a van der Pol wake oscillator coupled to the structure's
  acceleration (A = 12, ε = 0.3) reproduces lock-in with amplitudes of a
  few tenths of a diameter and a band several times the resonance width.
- **Proves:** a distributed 1-D field as one element (`line.string`,
  discretised at compile time) with ports at *positions* — a port family
  `tap.*` whose members an instance names — so a wake oscillator hangs on
  every cell; the taps hold their nodes to the string through reaction
  states that the initialisation solves.
- **Falsifier:** fix the shedding frequency — deafen the wake by setting
  its coupling to zero — and there is resonance but no lock-in; the plateau
  vanishes.

*Implementation note.* A 10 m, 0.1 m cable (T = 4 kN, 20 kg/m, first mode
0.707 Hz) in 8 cells, a `fluid.wake_oscillator` on each; twelve speeds from
0.55 to 2.2 times the nominal resonance speed, 40 s each, run twice — wake
listening and wake deaf. Observed: the deaf cable peaks at A/D = 0.24 and
holds half that peak over 4 of the 12 speeds; the listening cable peaks at
A/D = 0.61 and holds half its peak over 8 speeds, from 1.05 to 2.1 times the
nominal resonance speed. At the speed where St·U/D = 1.15 f₁ the listening
cable responds at 0.98 f₁ — on its mode, not on the shedding frequency —
and the response frequency walks from 0.58 f₁ below the band to 1.30 f₁
above it. Peak-band ratio 2.0, exactly at the threshold; the plateau's upper
edge is set by the wake model's coupling, so a stricter bound would need
Facchinetti's ε and A tuned to the measured cable.

---

### 24. Janssen's silo
`granular`

Pour grain into a tall silo and watch the stress on the floor. It rises
with the fill like a fluid's would, then stops rising. The walls carry the
rest through friction, and however much more you pour, the floor never
feels it.

- **Knob:** wall friction μ.
- **Change:** hydrostatic → saturating.
- **Number:** Janssen 1895: the floor stress saturates at `ρgD/(4μK)` over
  a depth scale `D/(4μK)`. Beverloo 1961: an orifice drains at
  `C·ρ√g·(D − k·d)^{5/2}`, whatever the load above — and not at all below
  `k ≈ 1.5` grain diameters.
- **Proves:** a granular connector `(stress | particle_flow)` with a column
  that carries Janssen's profile, a source, a Beverloo orifice that jams,
  and a sink; first-order filling on backward Euler so every recorded
  floor stress is consistent with the fill it belongs to.
- **Falsifier:** μ = 0 gives hydrostatic stress at every depth.

*Implementation note.* A 1 m silo of 1 500 kg/m³ grain (μ = 0.4, K = 0.5)
filled at 100 kg/s: saturation 18 394 Pa (exact to 10⁻³), depth scale
1.250 m from the fill curve; twice the grain, the same floor stress; μ = 0
hydrostatic to 10⁻⁶. Draining through a 0.1 m orifice: 7.091 kg/s with the
silo full and 7.091 kg/s half empty; a 3 cm opening on 2.5 cm grain: stuck.

---

### 25. Stochastic resonance
`translational` `stochastic`

A particle in a double well is pushed back and forth too weakly to cross
the barrier. Add thermal noise: with a little, nothing changes; with a lot,
it hops at random; in between the hops fall into step with the push and
the periodic signal comes out of the noise stronger than it went in.

- **Knob:** noise intensity (bath temperature).
- **Change:** no switching → synchronised switching → random switching.
- **Number:** Benzi, Sutera & Vulpiani 1981; Gammaitoni, Hänggi, Jung &
  Marchesoni 1998: the signal-to-noise ratio at the drive frequency peaks
  where two Kramers hops per period match the drive, `2r_K = Ω`.
- **Proves:** a through lane may carry a noise process — `Context::add_noise`
  with a declared intensity, drawn once per step and held through the
  implicit solve (drift implicit, diffusion explicit) — with a seeded
  generator in the runtime and `analysis::power_spectrum` to read the
  result.
- **Falsifier:** remove the second well and noise only ever hurts.

*Implementation note.* `translational.double_well` (a = b = 1, barrier
0.25) and `translational.langevin` (damping 1, a 0.1 drive at 0.05 rad/s —
26 % of the static threshold) on a 0.02 mass, 20 drive periods per run at
a fixed seed. The output power at the drive frequency is 68× the quiet
case at its best, at 0.7× the Kramers optimum (6 hops per period there,
10 at the optimum itself); the single well's output moves by a factor 2
across the same noise range and its signal-to-noise ratio only falls.

---

### 26. The double pendulum's two notes
`multibody` `joints`

Two equal rods hung one from the other. Rung gently it plays two notes at
once — in phase and counter-phase — and their ratio is fixed by nothing but
geometry. Weld the knee and one note is left, at the pitch of a single
stiff rod.

- **Knob:** initial swing.
- **Change:** two notes → one (welded); small swings → chaos (large).
- **Number:** for equal point masses on equal rods, `ω² = (2 ∓ √2)·g/L`
  (2.397 and 5.787 rad/s for L = 1 m); welded, `ω² = 3g/(5L)`.
- **Proves:** kinematic joints between owned frames — `joint.revolute`,
  `joint.fixed`, `joint.prismatic` — as constraint elements with
  multipliers, energy-conserving through the constraints, and swappable in
  a `ModelWorld` without touching the bodies.
- **Falsifier:** replace the knee's revolute joint by a fixed one: the
  counter-phase note vanishes and the remaining note moves to the stiff
  rod's.

*Implementation note.* `planar.rigid_body` rods with the mass at the tip,
`joint.revolute` at shoulder and knee (Baumgarte-stabilised velocity-level
constraints, two multipliers each), a massive pivot body held by
`joint.fixed`. Rung with the rods swung ±0.02 rad: both notes within 0.6 %,
energy drift 10⁻⁶ over 60 s; welded: one note within 1.2 %, the
counter-phase note gone. The multiplier joints make an index-2 pencil
whose finite eigenvalues from shift-invert move with rounding, so its
linearisation is recorded, not checked; the same pendulum authored on
`multibody.chain` (minimal coordinates, plates 31–32) linearises to both
notes within 10⁻⁶ (2.3972 and 5.7874 rad/s), and the ring's spectrum is
the evidence for the joints themselves. Item 12's compiler-level
elimination between separately authored bodies and 3-D joints remain
undone.

---

## Summary

| # | Test | Domains | Knob | Reference number |
|---|---|---|---|---|
| 1 | Kapitza's inverted pendulum | mechanical | drive frequency | a²Ω² > 2gL |
| 2 | Huygens' coupled clocks | mechanical, structural | beam stiffness | anti-phase lock, Bennett et al. 2002 |
| 3 | Current hogging | electrical, thermal | sign of dR/dT | loop gain < 1 for stability |
| 4 | Rijke tube | thermal, acoustic | gauze position | sings only in lower half, f = c/2L |
| 5 | Water hammer | hydraulic, structural | closure time vs. 2L/c | Δp = ρcΔv |
| 6 | Flutter | fluid, structural | airspeed | Theodorsen flutter speed |
| 7 | Dzhanibekov flip | multibody | which principal axis | λ = ω₂√((I₂−I₁)(I₃−I₂)/I₁I₃) |
| 8 | Tippe top | multibody, contact | spin rate | critical spin, Bou-Rabee et al. 2004 |
| 9 | Passive dynamic walker | multibody, contact | slope angle | θ* = 0.200310; period-doubling at γ ≈ 0.0151 |
| 10 | Euler buckling | structural | axial load | P_cr = π²EI/(KL)² |
| 11 | 2:1 spring pendulum | mechanical | spring stiffness | exchange at k/m = 4g/L |
| 12 | Stick–slip | mechanical, contact | belt speed | unstable iff −dF/dv > c |
| 13 | Backlash hunting | control, rotational | backlash width | amplitude ∝ b, frequency fixed |
| 14 | Sample-rate instability | control, electrical, rotational | sample period | K_p·K < coth(T/2τ) |
| 15 | Chua's circuit | electrical | resistor | Feigenbaum δ = 4.669 |
| 16 | Thermoelastic damping | structural, thermal | beam thickness | Zener Q⁻¹ peak at ωτ = 1 |
| 17 | Painlevé's paradox | multibody, contact | friction coefficient | jam iff μ > (1+3cos²θ)/(3 sinθ cosθ) |
| 18 | Motor hogging on composite plugs | composite, electrical, rotational, thermal | winding coefficient | |α|·R_th·P·R/(R+k²/c) = 1 |
| 19 | The Levitron | magnetic, multibody | spin rate | window edges from the linearised model; trap a/2 < z < a√0.4 |
| 20 | The geyser | fluid (two-phase), thermal | bottom heat | time to eruption ∝ 1/(heat − recharge cooling); 120 °C under 11 m |
| 21 | Semenov ignition | chemical, thermal | wall temperature | ψ = (E/RT_w²)·q_gen/hS = 1/e |
| 22 | Cooling below ambient | radiative, thermal | solar absorptivity | ≈ 5 K below ambient under the sun (Raman 2014) |
| 23 | VIV lock-in | line, fluid | flow speed | plateau ≥ 2× the resonance band; A/D of a few tenths (Facchinetti 2004) |
| 24 | Janssen's silo | granular | wall friction | floor stress saturates at ρgD/(4μK); Beverloo drain rate ignores the fill |
| 25 | Stochastic resonance | translational, stochastic | noise intensity | output at Ω peaks near 2r_K = Ω (68× the quiet case) |
| 26 | The double pendulum | multibody, joints | initial swing | ω² = (2 ∓ √2)·g/L; welded 3g/(5L) |
| 27 | Language independence | control, seam | controller (Rust / Python) | traces bit-identical; wall-clock term breaks it |
| 28 | Latency-induced instability | control, seam | bus latency (samples) | unit-circle crossing of `z^{d+1} − a z^d + L(1−a)`; 6.19 vs 6.26 at d = 2 |
| 29 | Quantisation hunt | control, sensing | encoder counts | relay describing function: A = 2q·|L(jω_c)|/π; 0.468 vs 0.469 counts |
| 30 | Missed deadlines | control, seam, real-time | compute time | boundary at the sample period; kept → decay, missed → growth |
| 31 | The leg on the seam | multibody, electrical, control, sensing | joint targets | hold current = gravity torque / (N·kt) to 1 %; old harness checks pass |
| 32 | The quadruped's trot | multibody, control, sensing, seam | stride | one stride per gait period net of the compliance creep (0.85 vs 0.60 m) |
| 33 | The scaling ladder | solver | rungs | cost per step ∝ unknowns^0.98 (dense would be 3); 75 % of unknowns solved |
| 34 | Cruise control on a hill | multibody, wheels, control, seam | grade | the integrator's torque on the hill = m·g·sin θ·r within 4 %; open loop the hill wins |
| 35 | Walk the plank | multibody, contact, seam, environment | curriculum level | the LIP planner crosses every level-0 course and fewer than half at 0.6; a 12 cm perception error brings it down; a restored snapshot replays bit for bit |

### 27. Language independence
`control` `seam`

The same PI speed law closes the same compiled motor loop three ways: as a
Rust closure inside the process, as a Python program in a child process
speaking the seam's frame protocol, and as that Python program again in a
second run. The three traces are identical to the last bit.

- **Knob:** which controller runs the law (in-process Rust, Python over
  the seam).
- **Change:** none — that is the point. A controller that reads its own
  clock is the change: two runs of it disagree.
- **Number:** worst difference between the Rust and Python traces, and
  between two Python runs: 0 rad/s (bit-identical); with a wall-clock term
  in the law, 6 × 10⁻³ rad/s between runs.
- **Proves:** `control.external` — the plant's side of the seam — with
  lockstep sampling, simulation-time-only frames, named unit-bearing
  channels derived from the wiring, and the newline-delimited JSON frame
  protocol of `sim-couple` with the stdlib-only Python client.
- **Falsifier:** a controller that mixes `time.perf_counter()` into its
  command cannot repeat itself.

*Implementation note.* Plant as plate 15's speed loop; PI with conditional
anti-windup at 2 ms, written once in Python (`clients/python/examples/
pi_controller.py`) and once as a `FnCoupler` closure with the same
operations in the same order. `serde_json` is built with exact float
round-tripping so a 17-digit actuator value survives the pipe.

---

### 28. Latency-induced instability
`control` `seam`

A proportional speed loop through the seam. The gain never changes; the
bus latency — whole samples between the sensor frame and the command —
does, and a loop stable at zero latency grows without bound three samples
later.

- **Knob:** bus latency in samples (`input_delay` on the seam).
- **Change:** decay → growth.
- **Number:** the zero-order-hold plant `x⁺ = a·x + b·u`, `a = e^{−T/τ}`,
  with `u[k] = −Kp·x[k − d]` has characteristic polynomial
  `z^{d+1} − a·z^d + L(1 − a)`; the loop goes unstable where a root reaches
  the unit circle. For τ = 45.8 ms, T = 5 ms the critical loop gains are
  18.3, 9.67, 6.26, 4.71, 3.83 for d = 0…4.
- **Proves:** sample-count delays as ring buffers of whole frames on the
  seam, and that the seam's samples land exactly on the sample instants.
- **Falsifier:** at the gain that tips the two-sample loop, the same loop
  with no latency decays at −279 /s.

*Implementation note.* Loop gain 5: stable for d ≤ 2, unstable from d = 3,
exactly as the polynomial says; bisecting the measured critical gain at
d = 2 gives 6.19 against the unit-circle crossing's 6.26 (1.1 %).

---

### 29. Quantisation hunt
`control` `sensing`

A PI position loop on a motor with an encoder, asked to hold a position
half a count from a count edge. With a continuous angle the loop settles;
with the angle quantised it never does — the encoder reads one count or
the next, never the target, the integrator winds back and forth, and the
shaft hunts for ever.

- **Knob:** encoder counts per turn (0 = continuous).
- **Change:** quiet → a sustained hunt of a fraction of a count.
- **Number:** at a count edge the quantiser is a relay of height `q/2`,
  describing function `N(A) = 2q/(πA)`; the hunt amplitude is
  `A = 2q·|L(jω_c)|/π` at the phase crossover `ω_c` of the linear part
  `L(s) = (Kp + Ki/s)·K/(s(1 + τs))·e^{−sT}` (two zero-order holds, the
  encoder's and the controller's).
- **Proves:** `sensor.encoder` — sample-and-hold with quantisation — and
  that a sampled sensor and a sampled controller due at the same instant
  both fire, the controller reading the sensor's fresh value.
- **Falsifier:** a continuous angle: the residual motion is below 10⁻⁷ rad.

*Implementation note.* T = 10 ms, Kp = 3 V/rad, Ki = 15 V/(rad·s):
|L(jω_c)| = 0.74 at 6.1 Hz, predicted amplitude 0.469 counts; measured
0.468 counts (1024 counts) at 8.2 Hz — a sampled relay cycle rounds to
whole samples. With 4096 counts the hunt shrinks to 0.27 of a (smaller)
count at the same frequency. Two library bugs surfaced here and were
fixed for everyone: two guards crossing in the same instant used to lose
the second (the located time sits a tolerance past the instant), and a
jump left the element's signal outputs stale until the next solve.

---

### 30. Missed deadlines
`control` `seam` `real-time`

Real-time mode: the Python controller of plate 27 runs behind the
`RealTime` coupler, which gives it one sample period of wall clock to
answer. Faster than the period it is invisible; slower, it misses every
deadline, its commands land a sample late, and a speed loop whose gain is
safe at zero latency but past the one-sample limit turns from decay to
growth.

- **Knob:** the controller's compute time.
- **Change:** deadlines kept and decay → deadlines missed and growth.
- **Number:** the boundary is the sample period: 0, 2 and 5 ms of compute
  time at a 10 ms period miss nothing and decay at −92 /s; 15 and 25 ms
  miss every sample and grow (+11.5 and +4.8 /s). The loop gain sits at
  the geometric mean of plate 28's zero- and one-sample limits at this
  period (9.20 and 5.10).
- **Proves:** the `RealTime` coupler — the controller on its own thread, a
  deadline per sample, a held command and a missed-deadline counter when
  it is late, the late answer applied at the next sample — which is the
  library's hardware-in-the-loop mode and the only deliberately
  non-deterministic thing in it.
- **Falsifier:** the same loop in lockstep has no notion of compute time.

---

### 31. The leg on the seam
`multibody` `electrical` `control` `sensing`

The robot leg that used to be a hand-assembled runtime, re-authored from
library parts and driven by a Python process: a three-link chain in
minimal coordinates, brushed motors behind PWM drivers with series current
sensors, ideal gears with compliant transmissions, encoders and
tachometers on the joints, heel and toe contacts under the foot.

- **Knob:** the joint targets the controller is given.
- **Change:** a pose step, a gravity hold, and — with the controller
  sending zeros — a leg that folds onto its foot.
- **Number:** holding the pose, each motor's current is the gravity torque
  at its joint over the reduction and torque constant: hip 3.05 A vs
  3.08 A, knee 3.28 vs 3.28, ankle 0.671 vs 0.671. The old harness's
  acceptance checks (pose error ≤ 0.12 rad, settled speed ≤ 0.35 rad/s,
  currents ≤ 18.5 A, gravity moves the passive leg ≥ 0.05 rad) all pass
  with margin: 0.0035 rad, 0.004 rad/s, 16.4 A peak, 0.30 rad.
- **Proves:** `multibody.chain` — the joint-elimination pass as an element:
  minimal coordinates, recursive Newton–Euler, joints exposed as
  rotational ports a motor plugs into, an owned tip frame contacts attach
  to — and the sensing domain's drivers and sensors composed with the
  seam into a whole robot.
- **Falsifier:** zero duty: the leg collapses 0.30 rad at the knee and the
  contacts carry it (23 N peak); the foot never passes the floor.

---

### 32. The quadruped's trot
`multibody` `control` `sensing` `seam`

A planar quadruped from library parts — a floating body, four two-link
chains hanging from two hips, servo joints, encoders and tachometers into
the seam, compliant contacts under the feet — and a Python process trotting
it: diagonal pairs alternate, stance feet sweep backward under their hips,
swing feet return along an arc, inverse kinematics turns foot targets into
joint targets, PD into torques.

- **Knob:** stride.
- **Change:** marching on the spot → walking.
- **Number:** kinematics: with stance feet planted the body advances one
  stride per gait period, 0.12 m × 5 = 0.60 m; walked 0.94 m, of which
  0.09 m is the creep the zero-stride march shows and the rest the same
  compliance ratchet under load — net 0.85 m.
- **Proves:** the whole seam stack on a floating-base robot: `multibody.chain`
  legs on a `planar.rigid_body`, `actuator.servo`, `sensor.encoder`/
  `sensor.tachometer`, `contact.point_plane_compliant`, `control.external`
  and the Python client, on the L-stable rule.
- **Falsifier:** zero stride: the body creeps 0.09 m in 3.5 s and stays at
  its standing height (0.518 of 0.526 m); pitch never exceeds 0.016 rad.
- **Same gait, second language:** the trot is also written in C
  (`clients/c/examples/quadruped_gait.c`, compiled on first use into
  `target/simloop/`); the robot walks the same walk — worst difference in
  body x over the run 0 m, to the bit — and the viewer runs the C
  controller when a compiler is at hand.

*Implementation note.* 12 kg body, 0.25 m links, PD 150/6 N·m per rad,
servo bandwidth 50 Hz, 4 ms sample period, gait period 0.6 s. Two things
were learnt on the way: PD legs sag under load and every re-planted foot
ratchets the body forward by the sag — stiffer joints shrink it, the
zero-stride march measures what is left — and four rigid unilateral
contacts landing at once defeat the smooth Newton where compliant contacts
on the backward Euler rule do not.

---

### 33. The scaling ladder
`solver`

Not a physical surprise but the number every large model depends on: how
the cost of a step grows with the size of the system. A ladder of `n`
inertias coupled by springs and dampers, a tachometer on every rung and a
torque at the top, compiled at six sizes and stepped.

- **Knob:** rungs (25 … 800; 104 … 3204 stored unknowns).
- **Change:** none in the physics — the top rotor spins up identically at
  every size — and everything in the cost.
- **Number:** seconds per step ∝ (unknowns)^0.98; dense factorisation would
  give 3. 0.10 ms per step at 25 rungs, 2.8 ms at 800. The solver carries
  75 % of the stored unknowns after the compiler eliminates the signal
  values and the rate lanes.
- **Proves:** sparse factorisation (`faer`) of the element-assembled
  Jacobian, the reduced solver vector behind a full `StateStore`, and
  the modified Newton's reuse across steps, together.
- **Falsifier:** `SIM_NO_REDUCE=1` keeps every unknown; the exponent holds
  (sparsity does that) and the constant rises.

---

### 34. Cruise control on a hill
`multibody` `wheels` `control` `seam`

A two-wheel car — a body on two `contact.wheel`s, the rear axle driven by
a servo the seam commands from a PI speed law on the axle tachometer — on
the flat and on a 6 % grade.

- **Knob:** the grade.
- **Change:** on the flat the loop holds speed with a few newton-metres;
  on the grade the integrator winds to the torque the hill demands and the
  speed does not move.
- **Number:** the change in steady axle torque between flat and grade is
  `m·g·sin θ·r` = 141 N·m; measured 136 N·m (3.7 %). Speed held within
  1 % in both cases.
- **Proves:** `contact.wheel` (rolling contact with an axle and regularised
  traction) and the `slope` of `planar.rigid_body`; a wheeled vehicle
  closed through the seam.
- **Falsifier:** hold the flat road's torque on the grade: the car loses a
  quarter of its speed in 12 s.

*Implementation note.* Sign convention worth fixing when there is time:
with the wheel's spin positive counter-clockwise (y up), positive axle
torque drives the body toward −x, so "forward" here is −x and the grade
torque appears with the opposite sign to the flat one; the check compares
magnitudes.

---

### 35. Walk the plank
`multibody` `contact` `seam` `environment`

A planar point-foot biped — a torso on two `multibody.chain` legs with
servos, encoders and tachometers behind a `control.external` seam that
runs a 1 kHz PD loop — on stepping stones and stairs generated from a
seed and a curriculum level (`contact.point_terrain_compliant`: horizontal
patches, nothing to stand on between them). The environment runs the
reduced-order stepping planner of Dai et al.'s "Walk the PLANC": the next
foothold is the next stone's centre, the step time comes from the linear
inverted pendulum's capture point (re-timed every policy period from the
measured one), the swing foot follows a smooth arc, the stance leg holds
the hip height and carries the torso's pitch regulation.

- **Knob:** the curriculum level, `[0, 1]`: gaps from 0.3 m to 0.7 m,
  stone heights to ±0.2 m, stair rises to 0.15 m.
- **Change:** at level 0 the planner alone walks every course; at 0.6
  fewer than half — the room a learner has, and the paper's premise.
- **Number:** the CLF on the LIP error averages 0.010 m² along a good
  walk and reads ten times that where the misled walk ends.
- **Proves:** the terrain element and the generator, the environment
  mode of the seam (reset, step, snapshot and restore — a restored
  snapshot replays the same trajectory to the bit), the planner as a
  usable teacher; and that a point-foot biped holding a posture topples
  within 2 s (no ankle: it must step).
- **Falsifier:** tell the planner the stones are 12 cm nearer than they
  are — a perception error — and it plants a foot in the gap.

*Implementation note.* Standing still on point feet defeated three
balance controllers in the time box; the plate states the fact instead.
Training lives in `clients/python/examples/planc` (numpy PPO, residual on
the planner's references, CLF reward, success curriculum) against the
`sim-gym` server; the plate runs the planner only.

## What the additions cover that the six didn't

- **The first slice itself.** 13 and 14 run on controller-to-gears with
  nothing new, so they can be the first phenomena in the suite rather than the
  last.
- **6-DOF and contact.** 7, 8, 9 and 12 exercise the multibody and contact
  crates, and 9 is a direct standard for the leg and quadruped work.
- **Determinism as a tested property.** 15 is the first scenario where
  reproducibility is the assertion, not a background assumption.
- **Entropy, not just energy.** 16 checks the thermal port's second-law
  bookkeeping, which the connector-layer design promises but nothing in the
  original six measures.
- **Bifurcations with exact locations.** 9, 10, 14 and 15 each give a
  threshold that can be computed independently of the simulator, so the pass
  criterion is a number, not a picture.
