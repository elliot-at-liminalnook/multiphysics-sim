# Worm-drive winch

A 12 V brushed DC motor turns a 30:1 worm gearbox that winds a 2 kg load
onto a Ø20 mm drum. The supply switches off at 1.2 s.

```sh
target/release/sim-spatial --system examples/systems-builder/worm-drive/winch.system.json
```

Press **R** to run and **Graphs** to open the dock. Select the drum to plot
its speed and angle, the motor to plot current and speed. With the worm
gearbox the drum stops dead when the power goes off. Select the gearbox,
click **Show alternatives** and swap to *Spur gearbox 30:1 (lossless)*
(same ports). Run again and the load now back-drives the shorted motor and
unwinds at about 3 rad/s.

## What the worm gear computes

`rotational.worm_gear` derives everything from gear geometry (module *m*,
starts *z₁*, teeth *z₂*, worm pitch diameter *d₁*, normal pressure angle
*φₙ*, friction *μ*):

| | |
|---|---|
| lead angle | tan λ = z₁·m / d₁ = 0.0625 → λ = 3.58° |
| ratio | N = z₂ / z₁ = 30 |
| forward efficiency | η = (cos φₙ − μ tan λ)/(cos φₙ + μ cot λ) = 45.4 % at μ = 0.07 |
| self-locking | μ ≥ cos φₙ tan λ = 0.0587 → yes (margin 1.19) |

Tooth forces follow the classical worm-gear analysis (Shigley §15-7). The
normal force *W* is the constraint's multiplier and the friction direction
follows the sliding speed. Directional efficiency and self-locking are
consequences of the force model, not special cases. The same contact model
(`sim_domain_rotational::helical`) drives `bridge.lead_screw`.

## Acceptance (`cargo test -p sim-runtime --test worm_drive`)

| check | simulated | closed form |
|---|---|---|
| motor speed while lifting | 800.04 rad/s | 800.04 rad/s |
| motor current | 1.1997 A | 1.1997 A |
| efficiency from simulated powers | η_sim = η | ±0.005 |
| drum after power-off (worm) | creeps 2.4e-4 rad in 0.6 s | holds (bound 2e-3) |
| drum after power-off (spur swap) | −3.0261 rad/s | −3.0267 rad/s |
| step 0.5 ms vs 0.25 ms | < 0.5 % | |

## Honest limits

- Motor, friction and coupling values are **estimates** (recorded as such in
  the file), not measured parts. Real worm friction falls with sliding speed
  (≈ 0.1 near stall to ≈ 0.02 at several m/s); the model uses one constant μ.
- Friction is regularised below ε = 0.01 rad/s at the worm, so a locked drive
  creeps at about ε·atanh(cos φₙ tan λ / μ) / N instead of stopping exactly.
- No backlash, tooth compliance, bearing or churning losses. The load is a
  constant torque plus its m·r² inertia; the rope never goes slack.
- Each gearbox has a shaft coupling (30 N·m/rad) between the motor and the
  worm. The solver allows one rigid inertia per shaft node, and real drives
  do have one. The snap suggestions flag the conflict if you try to put two
  inertias on one shaft.
