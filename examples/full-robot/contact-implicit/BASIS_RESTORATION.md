# More control freedom fits the grid but worsens the dense comparison

On the same 144 planning times, doubling the periodic controls from 32 to 64
reduces sampled balance error but does not improve the independently checked
motion. Dense force error is **.180520 N with 32 controls**, versus **.288824 N
with 64 controls**. Neither candidate passes physical qualification, and neither
is a measured faster gait. The prior 112-point result had .319302 N dense force
error, so the new 32-control result is useful progress at the same planned speed.

## Controlled experiment

The starting motion is `restore32-targeted.result.json`. Residual-based selection
retains its 112 check times and adds 32 missed violating times, giving 144. The
shared Rust periodic-spline refiner then doubles the controls while preserving
the position, velocity and acceleration curve to floating-point precision.
Both searches use the same 144 times, physical model, 30-iteration settings,
fixed displacement, duration and feasibility-only objective.

The independent 512-time initial comparison finds maximum differences of:

- position: 4.44e-16 m/rad;
- velocity: 2.71e-14 in the corresponding rates;
- acceleration: 1.09e-11 in the corresponding accelerations;
- base wrench: 5.03e-11 N/Nm;
- contact force: 2.22e-11 N.

These compare the actual physical frames, not only the control points. The
64-control parameterization has 1,150 free parameters versus 574. It uses about
twice as many model evaluations, so this is not a computational-speed benchmark
or proof about asymptotic solver performance.

Both cycle displacements remain fixed. Re-encoding the translated endpoint
against the refined first control changes each fixed value by −6.94e-18 m.
The adapter requires a bounded roundoff-sized change and records it explicitly;
it never widens the fixed bounds. The saved recipe's inherited prose about
unchanged displacement bounds should be read with its explicit
`fixed_displacement_roundoff` entries. The adapter's wording now makes that
qualification explicit. Planned displacement rate remains .2009265099 m/s to
the reported precision.

## Independent physical comparison

All rows below use 512 physical samples and 513 sampled CAD geometry poses.

| Trial | Dense force error N | Dense moment error Nm | Dense torque margin Nm | Loaded slip | Max interlink overlap µm |
| --- | ---: | ---: | ---: | ---: | ---: |
| Parent: 32 controls, 112 times | .319302 | .118825 | −.000369 | 89.906% | 16.91 |
| 32 controls, 144 times | **.180520** | **.069876** | −.026308 | 89.484% | 17.16 |
| 64 controls, 144 times | .288824 | .107247 | −.001924 | 89.895% | 16.91 |

At the 144 planning times, the 64-control force error is only .063255 N and
moment error .021363 Nm, compared with .180163 N/.051343 Nm for 32 controls.
This improvement does not survive the dense comparison. The larger basis has
more freedom to fit the chosen times; it also needs adequate temporal coverage.
Neither result establishes that its basis is sufficient or that further
physical improvement is impossible.

Both new searches reach their iteration limit. The 32-control trial uses 34,485
evaluations and the 64-control trial 69,046. Their initial objective costs agree
to about 1.1e-10 absolute. Final costs are 100.715 and 22.090, illustrating again
that a lower sampled objective does not imply better dense physical quality.

The .05 N force, .02 Nm moment, −.001 Nm torque-margin, 5% loaded-slip and zero
sampled interlink-overlap requirements are not all satisfied. Sampled point
penetration remains below 1 mm. No browser preset, authored CAD, runtime contact
law or user workspace state is changed.

## Slip and next work

The sampled loaded-slip ratio stays near 90% across the comparison. A derived
[conservative sampled slip bound](SAMPLED_SLIP_BOUND.md) makes explicit how
normal forces and material velocities can supply a schedule-free optimization
residual. The evidence reducer verifies that bound on every foot. It is not yet
a Rust control component and must not replace the actual slip gate or serve as
a global-speed certificate.

Next work should address slip directly while retaining force/torque feasibility
and independently checking collisions. Simply enlarging the motion basis or
accepting its lower sampled objective is not supported by these results.
Further temporal sampling remains necessary for any selected basis.

## Reproduction and verification

The recipe adapter now supports restoration recipes, an explicit iteration
budget and an explicit source result/initial artifact. It preserves the legacy
full-objective numerical recipe exactly. A regression also checks strict
restoration-schema fields, the 30-iteration override, retained 144 times and
bounded fixed-displacement roundoff. The initial physical-curve comparison is
saved in `restore64-targeted2-initial-parity.json`.

```sh
node examples/full-robot/contact-implicit/summarize_basis_restoration.mjs
```

This reproduces `restore-basis-summary.json`, checks the shared model/settings,
constant non-XY bounds, displacement agreement and check times, verifies exact
independent final reports and same-time dense physical frames, and checks the
sampled slip inequality. Source/binary identities are recorded in
`restore-basis-build-identities.json`. Only the refiner executable was rebuilt;
shared Rust physics and solvers are unchanged from the preceding tested build.
Both native searches and all audits completed before evidence indexing.
