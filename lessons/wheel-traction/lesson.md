---
title: Wheels and traction — when torque stops helping
summary: A rover wheel launches at full throttle and spins, then brakes and skids. A tyre can push only up to grip × the weight on it; past that, more torque just spins the wheel. How to find that limit, and how to drive right up to it.
order: 18
category: mechanisms
minutes: 20
requires: [gear-ratio]
systems:
  rover: rover.system.json
authors: [Systems builder]
teaches: [traction-limit, wheel-slip]
needs: [gear-ratio]
---
# Launching a rover

**By the end of this lesson you will be able to:**

- work out the most force a tyre can push with, and the fastest acceleration it allows;
- tell wheelspin and skidding apart from how fast the rim moves compared with the ground;
- choose a torque limit that uses the grip without wasting it.

A small rover slams its throttle open and its wheels spin, leaving it barely faster than a gentle start. Here is one of its four wheels: a [gearmotor](part:rover/rover/motor) driving an 80 mm [wheel](part:rover/rover/hub) that carries a quarter of the rover's weight, pushing its share of the [chassis](part:rover/rover/body).

## Grip has a limit

A tyre pushes the rover forward by pushing the ground backward, through friction. Friction can supply at most the **grip** coefficient μ times how hard the tyre is pressed onto the ground (the normal force N):

```text
F_max = μ·N
```

**Worked example.** Our wheel carries a quarter of a 4 kg rover: N = 1 × 9.81 = 9.8 N. Rubber on a dry floor, μ = {{param rover/rover/tyre.grip | 0.8}}: F_max = 0.8 × 9.8 = 7.8 N. On a dusty floor (μ = 0.3), only 2.9 N.

```sim-quiz
id: fmax
kind: numeric
question: A wheel carries {N} N of weight, on a floor with grip μ = 0.8. What is the most force it can push with, in newtons?
vary: { N: { min: 3, max: 25, step: 1 } }
answer_expr: 0.8 * N
tolerance: 2%
unit: N
hint: F_max = μ·N.
explain: "F_max = μ·N: for 9.8 N, 0.8 × 9.8 = **7.8 N**. More weight on a driven wheel means more grip. Each review asks with a different load."
concepts: [traction-limit]
```

## The fastest a wheel can accelerate

If every wheel is driven and each carries its share of the weight, the grip limits the whole rover's acceleration: a = F_max/m = μ·N/m, and with N = m·g that is simply μ·g, whatever the rover weighs.

**Worked example.** μ = 0.8: a_max = 0.8 × 9.81 = 7.8 m/s². From rest to 1 m/s takes at least 1/7.8 = 0.13 s. To push 7.8 N, the 40 mm-radius wheel needs 7.8 × 0.04 = 0.31 N·m, a fifth of what the gearmotor gives at stall (about 1.5 N·m).

```sim-quiz
id: predict-spin
kind: predict
scene: launch
question: At full throttle the gearmotor can put about 1.5 N·m on the wheel; grip needs only 0.31 N·m. What will the wheel do?
options:
  - { text: "Spin: the rim moves faster than the rover", correct: true, feedback: "Yes. The extra torque cannot push the rover harder; it spins the wheel up instead." }
  - { text: "Grip, and accelerate the rover five times faster than μ·g", feedback: "The ground cannot push harder than μ·N, however much torque there is." }
  - { text: "Stall", feedback: "Nothing holds the wheel still; the tyre slides on the floor." }
explain: "The rover accelerates at μ·g, while the rim races ahead of it: about 0.8 m/s at the rim at 0.1 s, against 0.4 m/s for the rover. That difference is **wheelspin**."
```

```sim-scene
id: launch
system: rover
title: Full throttle, then brake
caption: "Full throttle from 0.05 s: the wheel spins until the rover catches up with it. At 0.4 s the throttle drops to zero and the driver shorts the motor: the wheel locks up and skids. The ghost uses 25 % throttle."
companion: { label: "25 % throttle", set: { throttle.amplitude: 0.25 }, mode: ghost }
camera: { preset: front, zoom: 1.0, pitch: 0.25 }
run: { duration_s: 0.8, frame_rate: 250 }
script: launch.rhai
plots: [rover/body.axis.velocity, rover/hub.shaft.speed]
show: [forces, trails]
sliders:
  - { parameter: throttle.amplitude, label: "Throttle", min: 0.1, max: 1, step: 0.05 }
  - { parameter: rover/tyre.grip, label: "Grip μ", min: 0.1, max: 1.2, step: 0.05 }
hints:
  - "Lower the grip to 0.3 (dust): the launch takes much longer, and the wheel spins even at half throttle."
  - "Lower the throttle until the rim speed and the rover's speed stay together: that is the most torque the grip can use."
expect:
  - { observe: rover/body.axis.velocity, reduce: change, window: [0.07, 0.15], min: 0.55, max: 0.68, why: "While spinning, the rover accelerates at μ·g = 7.8 m/s²: 0.63 m/s in 0.08 s." }
  - { observe: rover/hub.shaft.speed, reduce: mean, window: [0.09, 0.11], min: 17, why: "At 0.1 s the rim moves at r·ω ≈ 0.8 m/s, twice the rover's 0.4 m/s: wheelspin." }
  - { observe: rover/body.axis.velocity, reduce: mean, window: [0.35, 0.4], min: 1.3, max: 1.4, why: "Top speed once gripping: r·ω₀ at the gearmotor's no-load speed, about 1.37 m/s." }
```

```sim-equation
id: slip-now
scene: launch
show: "slip = r·ω − v"
expr: r * w - v
result: { symbol: slip, unit: m/s }
terms:
  r: { symbol: r, unit: m, param: rover/rover/tyre.radius }
  w: { symbol: ω, unit: rad/s, observe: rover/hub.shaft.speed }
  v: { symbol: v, unit: m/s, observe: rover/body.axis.velocity }
caption: How much faster the rim moves than the ground under it. Positive while spinning (launch), negative while skidding (braking), near zero while the tyre grips.
```

The rover gains {{value scene=launch observe=rover/body.axis.velocity reduce=change window=0.07..0.15 | 0.62 m/s}} in 0.08 s: 7.8 m/s², exactly μ·g, while the wheel spins.

```sim-quiz
id: read-slip
question: "At 0.45 s, just after braking, the slip readout is negative: the rim moves slower than the rover. What is happening?"
options:
  - { text: "The wheel is skidding: braked harder than the grip can stop the rover", correct: true, feedback: "Yes. The shorted motor brakes the wheel faster than μ·g can slow the rover." }
  - { text: "The wheel is spinning backwards", feedback: "It still turns forward, just slower than the ground passes under it." }
  - { text: "The tyre is gripping perfectly", feedback: "Gripping would mean rim speed equals ground speed: slip near zero." }
moment: 0.45
explain: "Skidding is wheelspin in reverse. Grip limits braking exactly as it limits launching: the rover slows at μ·g at most."
concepts: [wheel-slip]
```

## Using the grip without wasting it

Spinning a wheel does not push the rover any harder. It wears the tyre, wastes energy as heat, loses steering, and on loose ground digs a hole. The rover here gets nearly the full μ·g even while spinning because this tyre model keeps its grip when sliding; real rubber usually loses 20–30 % of it, so a spinning wheel launches slower than a gripping one.

The fix is to limit the wheel torque to what the grip can use, μ·N·r. Robots do this with a current limit (torque is k·i), or by watching for the rim running ahead of the chassis and backing off: **traction control**.

```sim-quiz
id: torque-limit
kind: numeric
question: "A wheel of radius 0.04 m carries 9.8 N on a floor with μ = {mu}. What wheel torque uses all the grip without spinning, in N·m?"
vary: { mu: { min: 0.2, max: 1.0, step: 0.05 } }
answer_expr: mu * 9.8 * 0.04
tolerance: 2%
unit: N·m
hint: "Torque = force × radius, with force μ·N."
explain: "τ = μ·N·r: for μ = 0.8, 0.8 × 9.8 × 0.04 = **0.31 N·m**. On dust (μ = 0.3) only 0.12 N·m. A good limit adapts to the floor."
concepts: [traction-limit]
```

## Putting it in your own words

```sim-reflect
id: why-spin
prompt: A teammate's rover barely launches faster at full throttle than at half, and leaves black marks. Explain why in a few sentences, and what they could do.
model_answer: "A tyre can push only μ·N; for a wheel carrying its share of the weight that limits the rover to about μ·g of acceleration, whatever the motor can do. At half throttle the motor already makes more torque than μ·N·r, so at full throttle the extra torque only spins the wheel: the rim runs ahead of the ground, wearing the tyre (the marks) and wasting energy, and real rubber grips less while sliding. They should limit wheel torque to about μ·N·r, for example with a motor current limit or traction control that backs off when the wheels spin, or add weight on the driven wheels."
key_points:
  - { idea: "Grip limits force to μ·N (acceleration ≈ μ·g)", cues: ["μ·n", "μn", "μ·g", "grip", "friction"] }
  - { idea: "Extra torque only spins the wheel", cues: ["spin", "wheelspin", "rim", "faster than", "slip"] }
  - { idea: "Spinning wastes energy and wears the tyre", cues: ["wear", "heat", "waste", "marks", "energy"] }
  - { idea: "Limit torque: current limit or traction control", cues: ["current limit", "traction control", "limit torque", "back off", "weight"] }
```

## Going further

This part is optional.

**Weight transfer.** Accelerating tips a rover backward, pressing its rear wheels harder and lifting the front: rear-driven rovers launch better, and braking works mostly through the front wheels.

**Rolling resistance.** Even a gripping tyre loses a little: the rubber flexes as it rolls, costing about C_rr·N (here 1 % of 9.8 N). It is why the rover coasts to a stop on a flat floor.

```sim-compare
id: grip-sweep
system: rover
study: grip
title: Speed after 0.25 s of full throttle, on four floors
caption: "Grip, not the motor, sets the launch: speed at 0.25 s rises with μ until the motor's own limit takes over."
```

```sim-component
component: part.drive_wheel
show: [summary, equations, limits]
```

## Key ideas

- A tyre pushes at most **μ·N**; with the weight on driven wheels that caps acceleration near μ·g.
- Torque beyond μ·N·r only spins the wheel: rim speed r·ω runs ahead of the ground, the **slip**.
- Braking is limited the same way: past it the wheel skids.
- Limit wheel torque to the grip (current limit, traction control), or put more weight on the driven wheels.
