---
title: A DC motor's torque–speed line
summary: Why a brushed motor slows down and draws more current when you load it, built up one idea at a time — torque from current, voltage from spinning, then the balance between them.
order: 1
category: motors-and-drives
minutes: 25
systems:
  motor: motor.system.json
authors: [Systems builder]
teaches: [torque-constant, back-emf, voltage-budget, torque-speed-line, stall]
---
# One motor, one line

**By the end of this lesson you will be able to:**

- say what makes a motor's torque, and what makes its back-voltage;
- predict a motor's speed and current for a given load, without simulating;
- explain why a stalled motor draws the most current.

We will build this up one idea at a time. Each part ends with a short question, and the next part opens once you have answered it. Take your time: the questions are there to help the idea settle, not to test you.

## Meet the motor

A brushed DC motor has two sides. On the **electrical side**, a battery pushes current through a coil of wire, the winding. On the **mechanical side**, a shaft turns and drives whatever is attached to it, the load.

![A brushed DC motor as an electrical loop and a shaft, coupled by the same constant k](motor-model.svg "The electrical side (left) and the mechanical side (right). Everything in this lesson is about how they affect each other.")

That is all for now. No equations yet. Before we go further, notice which side is which.

```sim-quiz
id: two-sides
question: A motor lifts a small weight. Which of these belong to its mechanical side?
options:
  - { text: "The battery and the winding", feedback: "Those are the electrical side: they deal with voltage and current." }
  - { text: "The shaft and the weight it lifts", correct: true, feedback: "Yes. The mechanical side is about turning: the shaft, its speed, and the load." }
  - { text: "Only the magnets", feedback: "The magnets are what connect the two sides; we will come to them next." }
explain: The electrical side has the battery and the winding; the mechanical side has the shaft and the load. The motor's magnets link the two, and the rest of the lesson is about that link.
```

## Current makes torque

Before reading on, take a guess. You are not expected to know yet: trying first makes the explanation that follows easier to hold on to.

```sim-quiz
id: guess-torque
pretest: true
question: If you double the current in a motor's winding, what do you expect its torque to do?
options:
  - { text: "Double", correct: true, feedback: "That is what the next paragraph shows." }
  - { text: "Stay the same", feedback: "Keep this guess in mind and compare it with the next paragraph." }
  - { text: "Rise four times", feedback: "Keep this guess in mind and compare it with the next paragraph." }
concepts: [torque-constant]
```

Current flowing through the winding sits inside the motor's magnetic field, and the field pushes on it. That push turns the shaft. More current, more push: the torque is simply proportional to the current.

```text
τ = k·i          (torque τ in N·m, current i in A)
```

The number k is the motor's **torque constant**. For our motor, k = {{param motor.torque_constant | 0.012 N·m/A}}, read straight from the lesson's system file.

**Worked example.** With 2 A in the winding, the torque is τ = 0.012 × 2 = 0.024 N·m. Double the current to 4 A and the torque doubles too, to 0.048 N·m.

```sim-quiz
id: torque-from-current
kind: numeric
question: The same motor (k = 0.012 N·m/A) carries 3 A. How much torque does it make, in N·m?
answer: 0.036
tolerance: 2%
unit: N·m
hint: Multiply the current by k, just like the worked example.
explain: τ = k·i = 0.012 × 3 = **0.036 N·m**. Torque follows current, one for one.
```

## Spinning makes a voltage

Now run it the other way. If you spin the shaft by hand, the winding moves through the magnetic field, and a voltage appears across it. The motor has become a generator.

This voltage is called the **back-EMF**. It is proportional to speed, and, neatly, with the same constant k:

```text
back-EMF = k·ω          (speed ω in rad/s, back-EMF in V)
```

**Worked example.** Spinning at 500 rad/s (about 4800 rpm), the back-EMF is 0.012 × 500 = 6 V. At twice the speed it would be 12 V.

It is the same k because the same magnets and the same coil do both jobs. When the motor runs, this voltage appears as well, and it pushes back against the battery.

```sim-quiz
id: back-emf-at-speed
kind: numeric
question: The motor spins at 250 rad/s. What back-EMF does it make, in volts? (k = 0.012)
answer: 3
tolerance: 2%
unit: V
hint: Multiply the speed by k.
explain: back-EMF = k·ω = 0.012 × 250 = **3 V**. Half the speed of the worked example, half the voltage.
```

## The voltage budget

Here is how the two ideas meet. The battery's 12 V has to go somewhere, so think of it as a budget with two bills to pay:

- the back-EMF, k·ω, which the spinning motor makes;
- the rest, which pushes current through the winding's resistance R (2 Ω here): R·i.

```text
V = R·i + k·ω
```

**Worked example.** At 500 rad/s the back-EMF takes 6 V. That leaves 12 − 6 = 6 V to push current through 2 Ω, so i = 3 A, and the torque is 0.012 × 3 = 0.036 N·m.

So the faster the motor spins, the more of the budget the back-EMF takes, and the less current (and torque) is left.

Now finish a worked example yourself: the first step is done for you.

```sim-quiz
id: budget-steps
kind: steps
question: "The motor runs at 600 rad/s from 12 V (k = 0.012, R = 2 Ω). How much current flows?"
steps:
  - { prompt: "Back-EMF at 600 rad/s", worked: "k·ω = 0.012 × 600 = 7.2 V" }
  - { prompt: "Voltage left for the winding", answer: 4.8, unit: V, tolerance: 2% }
  - { prompt: "Current through 2 Ω", answer: 2.4, unit: A, tolerance: 2% }
explain: "12 − 7.2 = **4.8 V** across 2 Ω gives **2.4 A**. The next question leaves every step to you."
concepts: [voltage-budget]
```

```sim-quiz
id: current-at-speed
kind: numeric
question: The motor runs at 750 rad/s from 12 V. How much current flows, in amps? (k = 0.012, R = 2 Ω)
answer: 1.5
tolerance: 3%
unit: A
hint: "First the back-EMF, 0.012 × 750. Then whatever is left of the 12 V, divided by 2 Ω."
hints:
  - "Start with the back-EMF: how much of the 12 V does spinning at 750 rad/s take?"
  - "back-EMF = k·ω = 0.012 × 750 = 9 V. The rest of the 12 V drives the current."
  - "3 V are left for the winding: i = 3 V / 2 Ω."
explain: back-EMF = 9 V, leaving 12 − 9 = 3 V across 2 Ω, so i = **1.5 A**. Faster motor, less current.
concepts: [voltage-budget]
```

## Held still: stall

What happens if the shaft cannot turn at all? This is called **stall**: the load is too much for the motor, or something is jammed.

```sim-quiz
id: stall-current
question: The motor is held so it cannot turn (ω = 0). Using the voltage budget, what limits its current?
options:
  - { text: "The back-EMF", feedback: "At ω = 0 there is no back-EMF: k·ω = 0. That is exactly why a stalled motor draws so much." }
  - { text: "Only the winding resistance: i = V/R = 6 A", correct: true, feedback: "Right. With no back-EMF, the whole 12 V drives current through 2 Ω." }
  - { text: "The load torque", feedback: "The load sets the current only once the motor turns steadily. Held still, the electrical side alone decides: V = R·i." }
  - { text: "The winding's inductance", feedback: "Inductance only slows how fast current changes; it does not limit the steady value." }
hint: Put ω = 0 into V = R·i + k·ω.
explain: With ω = 0 the budget becomes V = R·i, so i = 12 V / 2 Ω = 6 A. The motor then makes its largest torque, τ = k·i = 0.072 N·m. This is the **stall torque**.
```

## Spinning free: no load

Now the opposite end. With nothing attached, the motor needs almost no torque, so almost no current flows. Then the back-EMF takes the whole budget. The speed where that happens is the **no-load speed**.

```sim-quiz
id: no-load-speed
kind: numeric
question: With no load, the current is zero and the back-EMF uses the whole supply of {V} V. What is the no-load speed ω₀? (k = 0.012 N·m/A; rad/s or rpm)
vary: { V: { min: 6, max: 14, step: 1 } }
given: { k: motor.torque_constant }
answer_expr: V / k
tolerance: 2%
unit: rad/s
hint: With no current, V = k·ω₀. Solve for ω₀.
explain: "V = k·ω₀, so ω₀ = V/k: at 12 V that is **1000 rad/s**, about 9500 rpm. Each review asks with a different supply voltage. Real motors fall a little short because friction is never quite zero."
check_with: { scene: load-step, observe: rotor.shaft.speed, reduce: mean, window: [0.2, 0.29], set: { supply.voltage: V, load.torque: "0" }, tolerance: 1% }
concepts: [back-emf, torque-speed-line]
```

## A straight line between the ends

You now know both ends: at standstill, 0.072 N·m of torque; spinning free, 1000 rad/s and no torque. In between, the voltage budget gives a straight line: every bit of extra speed takes a fixed slice of back-EMF, and so a fixed slice of current and torque.

![The motor's torque–speed line, two operating points and its power curve](torque-speed.svg "Every steady operating point of this motor lies on the teal line, from stall (top left) to no load (bottom right).")

**Worked example.** With a load of 0.02 N·m, the motor needs i = 0.02 / 0.012 = 1.67 A. That uses 2 × 1.67 = 3.33 V of the budget, leaving 8.67 V for back-EMF, so ω = 8.67 / 0.012 ≈ 722 rad/s. That is point A in the figure.

```sim-quiz
id: line-midpoint
question: The load is half the stall torque (0.036 N·m). Where on the line does the motor run?
options:
  - { text: "At about half the no-load speed, 500 rad/s", correct: true, feedback: "Yes. On a straight line, half the torque means half the way from stall to no load." }
  - { text: "At the no-load speed, 1000 rad/s", feedback: "Only with no load at all. Any load uses some of the budget for current." }
  - { text: "It stalls", feedback: "It stalls only when the load reaches the full 0.072 N·m." }
explain: Half the stall torque needs 3 A, which uses 6 V; the other 6 V is back-EMF at **500 rad/s**. That matches the worked example in "The voltage budget".
```

## Loading it

Time to see it happen. The scene starts the motor against a [load](part:motor/load) of 0.02 N·m, the point A we just worked out. At 0.3 s the load doubles. Before you watch, commit to a number: a prediction you have made yourself sticks far better than one you have been told.

```sim-quiz
id: predict-doubled
kind: predict
scene: load-step
question: The rotor first settles near 722 rad/s against 0.02 N·m. When the load doubles to 0.04 N·m, where will it settle? Work it out the same way as point A.
observe: rotor.shaft.speed
reduce: mean
window: [0.55, 0.6]
tolerance: 3%
unit: rad/s
explain: 0.04 N·m needs 3.33 A, which uses 6.67 V; the remaining 5.33 V is back-EMF at 5.33 / 0.012 ≈ **444 rad/s**, point B in the figure.
```

```sim-scene
id: load-step
system: motor
title: Load step at 0.3 s
caption: The rotor settles where motor torque equals the load; doubling the load moves it down the line.
camera: { preset: iso, zoom: 1.2 }
run: { duration_s: 0.6, frame_rate: 400 }
script: load-step.rhai
plots: [rotor.shaft.speed, motor.p.current]
phase: [{ x: rotor.shaft.speed, y: motor.p.current, title: "Operating point: current (∝ torque) against speed" }]
show: [forces, current]
sliders:
  - { parameter: supply.voltage, label: "Supply voltage", min: 6, max: 14, step: 0.1, unit: V }
  - { parameter: load.torque, label: "First load (negative: resists)", min: -0.06, max: 0, step: 0.002, unit: "N·m" }
hints:
  - "Lower the supply voltage: the whole line shifts down, so every load runs slower."
  - "Make the first load bigger (more negative): the first operating point moves toward stall."
  - "Hover over a chart to see that moment in the 3D view; press ← and → to jump between events."
challenge:
  goal: "Set the **supply voltage** so the rotor settles at **500 rad/s** against the first load (0.02 N·m), before the load doubles."
  hint: "Use the voltage budget. 0.02 N·m needs 1.67 A, which uses 3.33 V; 500 rad/s needs 6 V of back-EMF."
  win:
    - { observe: rotor.shaft.speed, reduce: mean, window: [0.25, 0.3], min: 490, max: 510, why: "The rotor settles within 10 rad/s of 500 before the load step." }
expect:
  - { observe: rotor.shaft.speed, reduce: mean, window: [0.25, 0.3], min: 708.0, max: 737.0, why: "Against 0.02 N·m the rotor settles near ω = (V − R·τ/k)/k ≈ 722 rad/s." }
  - { observe: rotor.shaft.speed, reduce: mean, window: [0.55, 0.6], min: 435.0, max: 454.0, why: "Doubling the load to 0.04 N·m drops the speed to ≈ 444 rad/s." }
  - { observe: motor.p.current, reduce: peak, window: [0.55, 0.6], min: 3.2, max: 3.5, why: "Current follows torque alone: i = τ/k ≈ 3.33 A, whatever the speed." }
```

Watch the dot on the operating-point plot below the scene: it slides along the straight line from A to B. The run agrees with the line: {{value scene=load-step observe=rotor.shaft.speed reduce=mean window=0.25..0.3 | 722 rad/s}} before the load step and {{value scene=load-step observe=rotor.shaft.speed reduce=mean window=0.55..0.6 | 444 rad/s}} after it.

```sim-equation
id: torque-now
scene: load-step
show: "τ = k·i"
expr: k * i
result: { symbol: τ, unit: N·m }
terms:
  k: { symbol: k, unit: N·m/A, param: motor.torque_constant }
  i: { symbol: i, unit: A, observe: motor.p.current }
holds: { observe: motor.case.torque, tolerance: 1% }
caption: The torque equation at the playhead. Scrub the scene and the numbers follow the run; the reaction on the motor's case matches k·i throughout.
```

```sim-quiz
id: current-follows-load
question: In the scene, the load doubled. What did the steady current do?
options:
  - { text: "It doubled, from 1.67 A to 3.33 A", correct: true, feedback: "Yes. τ = k·i, so twice the torque needs twice the current." }
  - { text: "It stayed the same, because the supply voltage did not change", feedback: "The voltage is fixed, but the back-EMF fell as the rotor slowed, leaving more of the 12 V to drive current.", remedy: fixed-voltage }
  - { text: "It rose a little, because the motor was working harder", feedback: "It rose by exactly the ratio of the torques. The torque equation τ = k·i has no speed in it at all.", remedy: tries-harder }
explain: Current is set by the torque the load demands, and nothing else. The speed then settles wherever the voltage budget balances.
moment: 0.45
```

```sim-remedy
id: fixed-voltage
misconception: "A fixed supply voltage means a fixed current"
body: "The supply voltage is fixed, but it is **shared**: V = R·i + k·ω. When the rotor slows, k·ω shrinks, so more of the same 12 V is left for R·i, and the current rises. Watch the current chart after 0.3 s: the voltage never moves, the current does."
scene: load-step
then: current-at-speed
```

```sim-remedy
id: tries-harder
misconception: "A loaded motor tries harder"
body: "A motor has no effort to give. Its torque is k·i, nothing else. When the load doubles, the rotor slows, the back-EMF falls, and the current rises until k·i equals the new load, exactly twice the old current. The dot on the operating-point plot slides down the same straight line; nothing about the motor changed."
scene: load-step
```

## Putting it in your own words

The motor does not "try harder" when it slows down. It simply makes less back-EMF, so more of the battery's voltage is left to drive current, and more current means more torque.

```sim-reflect
id: why-stall-hot
prompt: In two or three sentences, explain why a motor that is jammed (stalled) can burn out its winding, even though it is doing no mechanical work.
model_answer: With the rotor stopped there is no back-EMF, so the full supply voltage drives current through the winding's small resistance, here 6 A instead of under 2 A. The heat in the winding is I²·R, so it rises with the square of that current, about 13 times the running value, while none of the electrical power leaves as mechanical work.
key_points:
  - { idea: "Stalled means no back-EMF", cues: ["no back emf", "back emf+zero", "back emf+0", "not spinning+voltage", "stopped+back emf"] }
  - { idea: "So the current is only limited by the resistance (V/R)", cues: ["resistance", "v/r", "6 a", "largest current", "maximum current", "most current"] }
  - { idea: "Heat grows with the square of the current (I²R)", cues: ["i²r", "i^2", "i2r", "square", "heat"] }
  - { idea: "None of the power becomes mechanical work", cues: ["no work", "no mechanical", "all+heat", "nothing+out"] }
```

## Try it in the builder

Now use the line to design something. Open the task's copy of the motor in the builder, change what you think matters, and let the simulation judge.

```sim-task
id: fast-under-load
kind: design
scene: load-step
title: Keep the speed up under the doubled load
goal: "With a **9 V** supply the motor crawls once the load doubles. Change the design so that after the load step the rotor still turns at **420 rad/s or more**. Any parameter may change; the report shows what each attempt costs in current."
start: { supply.voltage: 9 }
win:
  - { observe: rotor.shaft.speed, reduce: mean, window: [0.55, 0.6], min: 420, why: "At least 420 rad/s against 0.04 N·m." }
report:
  - { label: "Speed after the step", observe: rotor.shaft.speed, reduce: mean, window: [0.55, 0.6], unit: rad/s }
  - { label: "Current after the step", observe: motor.p.current, reduce: mean, window: [0.55, 0.6], unit: A }
hints:
  - "Which term of the voltage budget does the doubled load fix, whatever you do?"
  - "The current is set by the load (i = τ/k). What is left to change is how much voltage is left over for back-EMF."
  - "More supply voltage, or less winding resistance, both leave more back-EMF and so more speed."
solution: { supply.voltage: 12 }
```

## Going further

This part is optional. It adds depth, not new essentials.

**The line as one formula.** Take the current out of the two equations and the torque at any speed is τ(ω) = (k/R)·(V − k·ω). From it, the stall torque is k·V/R and the no-load speed is V/k.

**Why the speed change is not instant.** The rotor's inertia and the winding resistance set a mechanical time constant J·R/k² ≈ 42 ms. In the scene, the slow-motion stretch after the load step shows that settling.

**The whole line, measured.** The system file keeps a sweep of the load from 0 to 0.06 N·m. Each row is one steady operating point; together they trace the line.

| Load τ (N·m) | Speed ω (rad/s) | Current i (A) | Power out τ·ω (W) |
|---:|---:|---:|---:|
| 0 | 1000 | 0 | 0 |
| 0.02 | 722 | 1.67 | 14.4 |
| 0.04 | 444 | 3.33 | 17.8 |
| 0.06 | 167 | 5.00 | 10.0 |

```sim-compare
id: load-sweep
system: motor
study: load_sweep
title: Speed and current against load
caption: Speed falls by 1/(k²/R) ≈ 13 900 rad/s per N·m; current rises by 1/k ≈ 83 A per N·m.
```

```sim-quiz
id: best-power
gate: false
question: Where on the line does this motor deliver the most mechanical power?
options:
  - { text: "At stall, where the torque is largest", feedback: "At stall ω = 0, so power τ·ω is zero however large the torque." }
  - { text: "At no load, where the speed is largest", feedback: "At no load τ = 0, so power is zero again." }
  - { text: "At half the no-load speed, about 500 rad/s", correct: true, feedback: "Yes: τ·ω is a parabola that peaks midway, at 18 W here." }
explain: Power τ·ω is zero at both ends of the line and largest at half the no-load speed (the dashed curve in the figure). Efficiency, though, is best nearer no load, so motors usually run between the two, and a gearbox picks the spot.
```

```sim-component
component: bridge.brushed_motor
show: [summary, equations, tradeoffs]
```

## Key ideas

- **Current makes torque:** τ = k·i.
- **Spinning makes a voltage:** back-EMF = k·ω, with the same k.
- **The voltage budget:** V = R·i + k·ω. Faster means more back-EMF and less current.
- Every steady point lies on one straight line, from stall (most torque, most current) to no load (fastest, no current).
- Stall is the hot corner: largest current, zero mechanical power.
