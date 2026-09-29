---
title: Motors are generators — back-driving and braking
summary: A spinning wheel turns a motor whose terminals are joined through a resistor. The motor becomes a generator, its current brakes the wheel, and the resistor decides how hard. The same physics lets a robot brake, and lets a pushed robot pump power back into its electronics.
order: 21
category: motors-and-drives
minutes: 20
requires: [motor-torque-speed]
systems:
  gen: generator.system.json
authors: [Systems builder]
teaches: [generator-braking]
needs: [back-emf, torque-constant]
---
# Push a motor, get a current

**By the end of this lesson you will be able to:**

- predict the current and braking torque of a back-driven motor;
- choose a resistance for a given braking speed and current;
- say where the braking energy goes.

Push a robot's leg by hand with its power off, and it resists more when its motor leads are joined together. Here a [wheel](part:gen/load) already spinning at 600 rad/s turns our 12 V [motor](part:gen/motor), whose terminals are joined through a [brake resistor](part:gen/brake).

## Spinning makes a voltage

You met this in the torque–speed lesson: turning a motor's shaft makes a back-EMF proportional to its speed, e = k·ω, with the same k as its torque constant.

**Worked example.** At 600 rad/s our motor (k = {{param motor.torque_constant | 0.012 N·m/A}}) makes 0.012 × 600 = 7.2 V. With nothing connected, that voltage just sits at the terminals and nothing else happens.

```sim-quiz
id: emf
kind: numeric
question: The wheel spins our motor at {w} rad/s with its leads open. What voltage appears at its terminals?
vary: { w: { min: 100, max: 900, step: 50 } }
given: { k: motor.torque_constant }
answer_expr: k * w
tolerance: 2%
unit: V
hint: e = k·ω.
explain: "e = k·ω: at 600 rad/s, **7.2 V**. The motor is a generator whether you want one or not. Each review asks with a different speed."
concepts: [back-emf]
```

## Close the circuit, feel the brake

Join the terminals through a resistance R_b and the back-EMF drives a current around the loop, through the winding (R_m) and the resistor. That current, in the motor, makes torque k·i, and it pushes against the motion: the faster the wheel, the harder the brake.

```text
i = k·ω / (R_m + R_b)          τ_brake = k·i
```

**Worked example.** R_m = 2 Ω and R_b = {{param brake.resistance | 1 Ω}}: at 600 rad/s, i = 7.2/3 = 2.4 A, and the brake torque is 0.012 × 2.4 = 0.029 N·m.

```sim-quiz
id: brake-steps
kind: steps
question: "The wheel turns the motor at 400 rad/s, its terminals joined through 1 Ω. How hard does it brake?"
steps:
  - { prompt: "Back-EMF, k·ω", worked: "0.012 × 400 = 4.8 V" }
  - { prompt: "Current through R_m + R_b = 3 Ω", answer: 1.6, unit: A, tolerance: 3% }
  - { prompt: "Brake torque, k·i", answer: 0.0192, unit: N·m, tolerance: 3% }
explain: "4.8/3 = **1.6 A**, and 0.012 × 1.6 = **19 mN·m**. Two thirds of the speed, two thirds of the torque: the brake fades as the wheel slows."
concepts: [generator-braking]
```

## Open, resistor, or short?

The scene brakes the wheel through 1 Ω; the companion joins the terminals directly (a short, 0.01 Ω).

```sim-quiz
id: predict-short
kind: predict
scene: brake
question: "Which stops the wheel sooner: the 1 Ω resistor, or a direct short?"
options:
  - { text: "The short", correct: true, feedback: "Yes. Less resistance, more current for the same back-EMF, more braking torque." }
  - { text: "The resistor: it absorbs the energy", feedback: "The resistor does absorb energy, but a short lets more current flow, and current is what brakes." }
  - { text: "They are the same: the same energy has to go", feedback: "The same energy, but it can leave faster or slower." }
explain: "Shorted, only the winding's 2 Ω limits the current (3.6 A at the start) and the wheel slows to about 160 rad/s by 0.5 s; through 1 Ω, to about 250 rad/s. Open, it would barely slow at all."
```

```sim-scene
id: brake
system: gen
title: A generator brake
caption: "The spinning wheel drives the motor as a generator into a 1 Ω resistor; the current brakes it. Right (purple): the terminals shorted."
companion: { label: "Terminals shorted", set: { brake.resistance: 0.01 }, mode: split }
camera: { preset: iso, zoom: 1.4, yaw: 0.95, pitch: 0.45 }
run: { duration_s: 2.0, frame_rate: 120 }
script: brake.rhai
plots: [load.shaft.speed, motor.p.current]
show: [power, current, heat]
sliders:
  - { parameter: brake.resistance, label: "Brake resistor", min: 0.01, max: 20, step: 0.01, unit: Ω }
hints:
  - "Make the resistor 20 Ω: the wheel coasts almost as if the leads were open."
  - "Braking torque is k²·ω/(R_m + R_b): the winding's own 2 Ω always limits it."
challenge:
  goal: "Choose a **brake resistor** that slows the wheel below **240 rad/s by 0.5 s**, without ever drawing more than **3 A**."
  hint: "The current is largest at the start: 7.2 V/(2 Ω + R_b). Keep it under 3 A, then brake as hard as that allows."
  win:
    - { observe: motor.p.current, reduce: peak, window: [0.0, 2.0], max: 3.0, why: "The current never exceeds 3 A." }
    - { observe: load.shaft.speed, reduce: mean, window: [0.49, 0.51], max: 240, why: "Below 240 rad/s at 0.5 s." }
expect:
  - { observe: load.shaft.speed, reduce: mean, window: [0.49, 0.51], min: 235, max: 260, why: "Braking time constant J·(R_m + R_b)/k² = 0.63 s, plus bearing friction: about 247 rad/s at 0.5 s." }
  - { observe: motor.p.current, reduce: peak, window: [0.0, 0.05], min: 2.3, max: 2.45, why: "At the start, k·ω/(R_m + R_b) = 7.2/3 = 2.4 A (flowing backwards: generating)." }
```

```sim-equation
id: current-now
scene: brake
show: "i = −k·ω / (R_m + R_b)"
expr: -k * w / (Rm + Rb)
result: { symbol: i, unit: A }
terms:
  k: { symbol: k, unit: N·m/A, param: motor.torque_constant }
  w: { symbol: ω, unit: rad/s, observe: load.shaft.speed }
  Rm: { symbol: R_m, unit: Ω, param: motor.resistance }
  Rb: { symbol: R_b, unit: Ω, param: brake.resistance }
holds: { observe: motor.p.current, window: [0.01, 2.0], tolerance: 2% }
caption: "The generated current at the playhead (negative: flowing out of the motor's + terminal). It falls with the speed, and so does the braking."
```

Through 1 Ω the wheel is down to {{value scene=brake observe=load.shaft.speed reduce=mean window=0.49..0.51 | 247 rad/s}} after half a second.

## How fast it stops

The braking torque is proportional to speed, like viscous friction, so the speed decays exponentially, with a time constant set by the inertia J, the loop's resistance and k:

```text
τ_stop = J·(R_m + R_b) / k²
```

**Worked example.** J = {{param load.inertia | 0.00003 kg·m²}}, R_m + R_b = 3 Ω, k² = 0.000144: τ_stop = 0.00003 × 3/0.000144 = 0.63 s. Shorted (2 Ω): 0.42 s. The winding's own resistance sets the hardest brake possible.

```sim-quiz
id: tau-stop
kind: numeric
question: With a brake resistor of {Rb} Ω, what is the braking time constant, in seconds? (J = 0.00003 kg·m², R_m = 2 Ω, k = 0.012 N·m/A.)
vary: { Rb: { min: 0, max: 10, step: 0.5 } }
given: { J: load.inertia, k: motor.torque_constant, Rm: motor.resistance }
answer_expr: J * (Rm + Rb) / (k * k)
tolerance: 3%
unit: s
hint: τ_stop = J·(R_m + R_b)/k².
explain: "τ_stop = J·(R_m + R_b)/k²: for 1 Ω, **0.63 s**. More resistance, gentler braking. Each review asks with a different resistor."
concepts: [generator-braking]
```

## Where the energy goes

The wheel's kinetic energy, ½·J·ω² = ½ × 0.00003 × 600² = 5.4 J, has to go somewhere. The same current flows through the winding and the resistor, so it divides in proportion to their resistances: R_b/(R_m + R_b) of it heats the resistor, the rest the motor.

```sim-quiz
id: energy-split
question: "Braking through 1 Ω (winding 2 Ω), roughly how does the 5.4 J divide?"
options:
  - { text: "About a third in the resistor, two thirds in the winding", correct: true, feedback: "Yes: the same current through 1 Ω and 2 Ω, so heat in the ratio 1 : 2 (a little also goes to bearing friction)." }
  - { text: "All of it in the resistor", feedback: "The current also flows through the winding, which has more resistance here." }
  - { text: "None: braking destroys the energy", feedback: "Energy is never destroyed; here it all becomes heat." }
explain: "The shorter the resistor, the harder the brake, and the more of the heat lands in the motor itself. A large brake resistor keeps the motor cool, at the cost of a gentler brake."
concepts: [generator-braking]
```

## Putting it in your own words

```sim-reflect
id: why-brake
prompt: Explain, in a few sentences, why a robot's leg is harder to push by hand when its motor's leads are shorted than when they are open, and where the effort you put in ends up.
model_answer: "Turning the motor makes a back-EMF k·ω. With the leads open no current flows, so there is no torque. Shorted, that voltage drives a current limited only by the winding's resistance, and the current makes a torque k·i that opposes the motion, proportional to the speed (k²·ω/R). So the faster you push, the harder it resists. The work you do becomes heat in the winding (and in any resistor in the loop), in proportion to their resistances."
key_points:
  - { idea: "Turning the motor makes a back-EMF k·ω", cues: ["back-emf", "back emf", "k·ω", "generator", "voltage"] }
  - { idea: "Closing the circuit lets current flow", cues: ["current", "short", "closed", "circuit", "loop"] }
  - { idea: "The current's torque opposes the motion, growing with speed", cues: ["opposes", "brake", "k·i", "faster", "speed"] }
  - { idea: "The energy becomes heat in the winding and resistor", cues: ["heat", "i²r", "resistor", "winding", "energy"] }
```

## Going further

This part is optional.

**Regenerative braking.** Instead of a resistor, a driver can push the generated current back into the battery, recovering some of the energy. The catch: a supply that cannot take current back, such as most bench supplies, or a battery disconnected by a protection circuit, sees its voltage climb instead. Robots that are pushed or dropped can over-volt their own electronics this way; drivers add a "brake chopper" resistor or a clamp to catch it.

**The same as friction?** A shorted motor brakes like viscous friction (torque ∝ speed): strong at speed, fading to nothing at standstill. It can slow a load but never hold it; that needs a mechanical brake or a self-locking gear.

```sim-compare
id: resistor-sweep
system: gen
study: resistor
title: Speed after 0.5 s, for five brake resistances
caption: "From shorted (0.01 Ω) to open (1 MΩ): the brake fades as the resistance grows."
```

```sim-component
component: bridge.brushed_motor
show: [summary, equations]
```

## Key ideas

- A back-driven motor is a generator: e = k·ω.
- Close its circuit and the current brakes it: τ = k·i = k²·ω/(R_m + R_b). This is **generator braking**.
- Speed decays with time constant J·(R_m + R_b)/k²; a short gives the hardest brake.
- The energy becomes heat, split between winding and resistor in proportion to their resistances.
