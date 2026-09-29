---
title: Springs in series — measuring and cushioning force
summary: A servo swings an arm into a wall, once through a stiff coupling and once through a soft spring. The spring's twist measures the torque, and it keeps the gearbox's own heavy rotor from slamming into the wall.
order: 13
category: mechanisms
minutes: 15
requires: [gear-ratio]
systems:
  arm: arm.system.json
authors: [Systems builder]
teaches: [spring-torque, series-elastic]
needs: [rotational-inertia, reflected-inertia]
---
# The arm meets a wall

**By the end of this lesson you will be able to:**

- work out a torsion spring's torque from its twist, and use it as a torque sensor;
- explain why a series spring protects a gearbox when a limb hits something;
- say what a softer spring costs.

Walking robots hit things all the time: every step is a small collision with the ground. Geared motors are not built for it. Here a [servo](part:arm/servo) swings an [arm](part:arm/arm) through a [spring](part:arm/spring) into a [wall](part:arm/wall) at 0.8 rad. Behind the servo sits its gearbox, and the [motor rotor](part:arm/rotor) as the arm sees it.

## Spring torque

A torsion spring twists in proportion to the torque across it. Twist one end against the other by Δθ and it pushes back with:

```text
τ = k·Δθ          (k in N·m/rad)
```

**Worked example.** Our spring has k = {{param spring.stiffness | 3 N·m/rad}}. Twisted by 0.2 rad (11°), it carries 3 × 0.2 = 0.6 N·m.

```sim-quiz
id: twist
kind: numeric
question: Our spring (k = 3 N·m/rad) is twisted by {dt} rad. What torque does it carry, in N·m?
vary: { dt: { min: 0.05, max: 0.5, step: 0.01 } }
given: { k: spring.stiffness }
answer_expr: k * dt
tolerance: 2%
unit: N·m
hint: τ = k·Δθ.
explain: "τ = k·Δθ: at 0.2 rad, 3 × 0.2 = **0.6 N·m**. Each review asks with a different twist."
concepts: [spring-torque]
```

## A torque sensor made of a spring

Put an encoder on each end of the spring, and the difference of their angles is its twist. Multiply by k and you know the torque passing through, without a load cell. That arrangement, motor then spring then load, is a **series-elastic actuator**.

![Motor and gearbox, a spring, then the arm, with an encoder on each side of the spring](sea.svg "The spring's twist between the two encoders gives the torque: τ = k·(θ_motor − θ_arm).")

```sim-quiz
id: sensor-choice
question: A series-elastic joint reads θ_motor = 1.10 rad and θ_arm = 0.80 rad, with k = 3 N·m/rad. What is the arm pushing against?
options:
  - { text: "0.9 N·m, in the direction the motor is turning", correct: true, feedback: "Yes: a twist of 0.3 rad × 3 N·m/rad." }
  - { text: "3.3 N·m: k times the motor angle", feedback: "Only the twist, the difference between the two ends, loads the spring." }
  - { text: "Nothing: the arm is not moving", feedback: "A still arm can still be pushing hard, as against a wall. The twist says how hard." }
explain: "Twist = 1.10 − 0.80 = 0.3 rad, so τ = 3 × 0.3 = **0.9 N·m**. The spring converts force into an angle that two encoders can read."
concepts: [series-elastic]
```

## Into the wall

The servo's target sweeps to 1.2 rad, past the wall at 0.8 rad. The companion run replaces the soft spring with a stiff coupling (300 N·m/rad).

```sim-quiz
id: predict-impact
kind: predict
scene: wall
question: The arm hits the wall at about 1.2 rad/s both times. Which run slams the wall harder?
options:
  - { text: "The stiff coupling", correct: true, feedback: "Yes, several times harder. The next part explains why the difference is so large." }
  - { text: "The soft spring: it stores energy and releases it into the wall", feedback: "A spring does store energy, but it also stretches out the stop. Watch the torque charts." }
  - { text: "Neither: the arm is the same, so the impact is the same", feedback: "The arm is the same, but what stands behind it is not." }
explain: "With the stiff coupling the wall sees a spike of about 4.7 N·m; through the soft spring, never more than {{value scene=wall observe=wall.shaft.torque reduce=max window=0.7..1.5 | 1.19 N·m}}."
```

```sim-scene
id: wall
system: arm
title: Soft spring against a stiff coupling
caption: "The arm swings into the wall at 0.8 rad. Left: through a 3 N·m/rad spring. Right (purple curves): a 300 N·m/rad coupling."
companion: { label: "Stiff coupling (300 N·m/rad)", set: { spring.stiffness: 300 }, mode: split }
camera: { preset: top, zoom: 1.4 }
run: { duration_s: 1.5, frame_rate: 400 }
script: wall.rhai
plots: [wall.shaft.torque, spring.a.torque]
show: [forces]
sliders:
  - { parameter: spring.stiffness, label: "Spring stiffness", min: 1, max: 300, step: 1, unit: "N·m/rad" }
hints:
  - "Try 10, 30 and 100 N·m/rad: the peak torque in the spring rises steadily."
  - "At 300 the peak torque through the coupling is more than twice the servo's 2 N·m rating: in a real servo, stripped teeth."
challenge:
  goal: "Choose a **stiffness** that keeps the torque through the gearbox (the spring's) under **1.3 N·m** at every moment, while still pressing on the wall."
  hint: "Soft enough that the rotor's momentum is soaked up by twisting the spring."
  win:
    - { observe: spring.a.torque, reduce: max, window: [0.0, 1.5], max: 1.3, why: "The gearbox never carries more than 1.3 N·m." }
    - { observe: wall.shaft.torque, reduce: mean, window: [1.3, 1.5], min: 0.5, why: "The arm still presses on the wall at the end." }
expect:
  - { observe: wall.shaft.torque, reduce: max, window: [0.7, 1.5], min: 1.1, max: 1.4, why: "Through the soft spring the wall never feels much more than the final steady push: about 1.2 N·m at most." }
  - { observe: spring.a.torque, reduce: max, window: [0.0, 1.5], min: 1.0, max: 1.3, why: "The gearbox side never carries more than about 1.2 N·m (the stiff coupling: 4.6 N·m)." }
  - { observe: spring.a.torque, reduce: mean, window: [1.3, 1.5], min: 1.05, max: 1.12, why: "At rest against the wall, the spring holds τ = k·Δθ = 3 × 0.363 ≈ 1.09 N·m." }
```

```sim-equation
id: spring-now
scene: wall
show: "τ = k·(θ_motor − θ_arm)"
expr: k * (a - b)
result: { symbol: τ, unit: N·m }
terms:
  k: { symbol: k, unit: N·m/rad, param: spring.stiffness }
  a: { symbol: θ_motor, unit: rad, observe: servo.shaft.angle }
  b: { symbol: θ_arm, unit: rad, observe: arm_inertia.shaft.angle }
holds: { observe: spring.a.torque, tolerance: 1% }
caption: The spring as a torque sensor, at the playhead. After the arm stops, the servo keeps winding the spring up until its twist carries about 1.09 N·m.
```

## Why the stiff one hits so hard

Behind the servo's output is a 100:1 gearbox, and behind that a rotor. From the arm's side, that rotor looks 100² times heavier: 0.05 kg·m², twelve times the arm itself. With a stiff coupling, when the arm stops at the wall, the rotor must stop with it, almost instantly, and its momentum goes straight through the gear teeth.

With a soft spring, the arm stops but the rotor does not have to. It keeps turning, winding the spring up, and slows down over a tenth of a second instead of a millisecond. The gearbox never feels the jolt.

```sim-quiz
id: why-soft
question: What does the soft spring change in the collision?
options:
  - { text: "It lets the heavy rotor slow down gradually, instead of stopping with the arm", correct: true, feedback: "Yes. The spring decouples the rotor's momentum from the arm's sudden stop." }
  - { text: "It makes the arm lighter", feedback: "The arm is the same 0.3 kg, and the bumper still stops it. What changes is whether the rotor stops with it." }
  - { text: "It absorbs the energy and turns it into heat", feedback: "An ideal spring stores energy, not heat. What matters is that it spreads the rotor's stop over a longer time." }
explain: "Impact force comes from stopping momentum quickly. The spring lets the rotor's much larger momentum be stopped slowly, while only the light arm stops suddenly."
moment: 0.78
concepts: [series-elastic]
```

## The price of softness

A soft spring is a sloppy connection. To move the arm, the motor must first wind the spring up; to stop, unwind it. Arm and spring also bounce against each other at their natural frequency, √(k/J_arm): {{param spring.stiffness | 3 N·m/rad}} on 0.0043 kg·m² rings at about 26 rad/s (4 Hz). Position control through it is slower and needs care, and precise fast moves become hard.

```sim-quiz
id: price
question: A designer halves the spring's stiffness to protect the gearbox better. What gets worse?
options:
  - { text: "Fast, precise position control: the joint responds more slowly and bounces at a lower frequency", correct: true, feedback: "Yes: √(k/J) falls by √2, so the controller must be gentler." }
  - { text: "The torque measurement: a softer spring is less accurate", feedback: "A softer spring twists more for the same torque, which makes small torques easier to read, not harder." }
  - { text: "Nothing: softer is always better", feedback: "A softer spring is kinder in collisions but lowers the joint's natural frequency and bandwidth." }
explain: "Softer springs cushion better and measure small torques more finely, but lower the natural frequency √(k/J) and so the speed of precise control. Real designs pick the softest spring the task's speed allows."
concepts: [series-elastic]
```

## Putting it in your own words

```sim-reflect
id: why-sea
prompt: Explain to a colleague, in a few sentences, why some walking robots put a spring between each geared motor and its leg, and what they give up for it.
model_answer: "Behind a high-ratio gearbox, the motor's rotor looks N² times heavier to the leg. When the foot hits the ground, a rigid drive has to stop that large reflected inertia almost instantly, and the shock goes through the gear teeth. A series spring lets the rotor slow down gradually while only the light leg stops suddenly, so peak gear torques fall several times. The spring also measures torque: its twist between two encoders times k. The cost is a softer, bouncier joint with a lower natural frequency, so fast precise motion is harder to control."
key_points:
  - { idea: "The rotor's reflected inertia is large behind a gearbox", cues: ["reflected", "n²", "n^2", "rotor", "gearbox", "inertia"] }
  - { idea: "The spring lets it slow gradually, cutting peak torque", cues: ["gradual", "slowly", "cushion", "peak", "shock", "impact"] }
  - { idea: "Twist times k measures torque", cues: ["twist", "k·Δθ", "k*", "sensor", "measure", "encoder"] }
  - { idea: "Cost: softer, lower bandwidth, bouncier", cues: ["bandwidth", "slower", "bounce", "natural frequency", "sloppy", "soft"] }
```

## Going further

This part is optional.

**Torque control through the spring.** Because torque is k·Δθ, a controller can hold a chosen torque by holding a chosen twist, which is a position loop on the motor side. Series-elastic arms use this to be gentle around people.

**Energy storage.** A spring also stores energy, ½·k·Δθ²: at the wall, 0.5 × 3 × 0.363² ≈ 0.2 J. Running and hopping robots use tendon-like springs to return landing energy on the next push-off.

```sim-component
component: rotational.spring
show: [summary, equations, tradeoffs]
```

## Key ideas

- A torsion spring carries **τ = k·Δθ**; its twist between two encoders is a torque sensor.
- A **series-elastic actuator** puts that spring between the geared motor and the load.
- In a collision it lets the rotor's large reflected inertia slow gradually: peak gear torque falls several times.
- The price is a softer, slower, bouncier joint, with natural frequency √(k/J).
