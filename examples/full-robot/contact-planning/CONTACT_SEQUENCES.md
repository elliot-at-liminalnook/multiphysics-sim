# Multiple contact steps in the shared motion representation

The shared Rust `ContactPhaseMotion` now accepts independent additional steps
for each foot. Each step declares touchdown phase, stance duration, foothold
center, three-dimensional swing excursion and optional return shaping. Feet
may have different step counts. This removes the one-stance/one-swing restriction
from geometric references and point-force CAD planning, while keeping saved
single-step JSON and its numerical sampling path unchanged.

`additional_steps` is optional and omitted when empty. Durations and onsets are
fractions of the common body cycle. Each stance must end before that foot's next
touchdown, including across the cycle boundary. Coincident transitions of
different feet remain supported. Steps are sorted for sampling without changing
their configuration indices. Foothold positions use the existing midpoint drift
convention: center + cycle displacement * (cycle number + onset + stance/2).
During stance that world position is constant. Each C2 swing connects the
current foothold to the next independently declared foothold.

Body motion, reverse-clock chain rules and Rhai sampling continue through the
same shared implementation. The CAD planner checks all step centers against
the declared floor and samples every stance/swing interior and contact interval.
Additional-step phase, duration, center and excursion variables are available
to its motion optimizer. No CAD, actuator, collision or walking gate is relaxed.

## Verified initialization and behavior

`ContactPhaseConfig::repeated_cycle(count)` expands a motion into a longer common
cycle, retaining its speed and trajectory while making the extra steps
independently adjustable. Its 64-repeat allocation cap is a software bound.
The geometric tests cover independent footholds, stationary stances, unequal
step counts, wrapping contacts, every transition, numerical derivatives,
reverse-clock derivatives, invalid configurations and legacy serialization.
Repeated-cycle equivalence also uses a nonconstant six-channel body spline,
both single/multiple-step inputs, negative times and more than 1,300 samples per
case. The existing Rhai contract executes a two-step foot through the same API.

The CAD comparison expands the command-bounded low-speed seed from four total
steps to eight. The original 128 frames and expanded 256 frames agree within
2.676e-12 across compared numeric frame fields, including joint motion, forces,
torques, command limits and geometry. Each original sample matches two expanded
samples; normalized quadrature weights halve. Both pass the unchanged sampled
planner at **0.025795773709373453 m/s**. This is preservation of a starting motion,
not a speed improvement or executed multi-step walking result.

The source recipe derives from `joint-mesh-command-best.recipe.json`; this
comparison uses the planner's point-force allocator, not the saved conic force
curves. The full inputs, reports and [comparison](contact-sequence-equivalence.json)
are versioned with a [hashed evidence manifest](contact-sequence-evidence.json).
`check_contact_sequences.mjs` checks physical-configuration
identity, phase/clock coverage, feasibility and numerical equivalence with a
declared 1e-8 absolute comparison tolerance. It writes a fresh result exclusively.
Existing simulation CI runs the shared control, script and planner tests.

## Remaining integration for speed search

The [per-stance joint-force integration](JOINT_STEPS.md) now supports independent
force templates, event identities, force Jacobians and conic rows. Incomplete
template arrays still reject rather than silently ignoring additional contacts.

The expanded seed is now in a joint search that frees per-step loads,
placements, durations and body motion together. Next compare contact-family
selection against matched multistart. Broader search is not evidence that a
physical speed maximum has been reached. Timestep, sustained walking, WASD and
browser validation remain required for any improved candidate.

## Reproduction

Build `evaluate_contact_sequence` with `cargo build --release -p sim-runtime
--example evaluate_contact_sequence`. The CLI takes scene, capture-marker and
`{robot,motion}` recipe JSON, then emits a planning report. Its `--repeat recipe
count` mode emits an expanded recipe without altering physical robot properties.
For the paired audit, the expanded recipe explicitly doubles uniform sample
count and duplicates additional phases at half-cycle offsets. Evaluate both
saved recipes with the CAD scene/markers referenced by the evidence manifest,
then run the comparison script in a fresh output checkout or preserve its
existing result before reproduction. The CLI does not actuate hardware.
