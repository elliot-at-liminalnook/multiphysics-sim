---
title: Rotary to linear — belts, racks and screws
summary: The same small motor tries to lift 2 kg straight up, first with a belt, then with lead screws of two pitches. Every linear drive is a gear ratio in radians per metre, and that one number predicts the speed, the force and what happens when the power goes off.
order: 17
category: mechanisms
minutes: 25
requires: [gear-ratio]
systems:
  belt: belt.system.json
  screw: screw.system.json
authors: [Systems builder]
teaches: [linear-ratio]
needs: [gear-ratio, torque-speed-line]
---
# A lift for 2 kg

**By the end of this lesson you will be able to:**

- turn a motor's torque and speed into a linear drive's force and speed;
- tell from one number whether a drive can lift a given load;
- choose between a belt and a lead screw for a vertical axis.

A robot's lift, a gripper's jaws, a 3D printer's axes: all turn a motor's rotation into straight travel. Here our 12 V can [motor](part:belt/motor) tries to lift a 2 kg [carriage](part:belt/carriage) (19.6 N of [weight](part:belt/weight)) with a [GT2 belt](part:belt/belt), then with a [lead screw](part:screw/screw).

## Every linear drive is a ratio

A belt on a pulley of radius r moves r metres for every radian the pulley turns. A lead screw moves its nut one lead for every full turn. Either way, a fixed number of radians buys a metre of travel. Call it the drive's **linear ratio** G, in rad/m. It plays exactly the part a gear ratio does: the speed is divided by it.

```text
v = ω / G
```

**Worked example.** Our GT2 pulley has r = {{param belt/belt.radius | 0.00637 m}}: G = 1/0.00637 = 157 rad/m. At 600 rad/s the carriage moves 600/157 = 3.8 m/s. A screw with an 8 mm lead has G = 6.283/0.008 = 785 rad/m: at 600 rad/s, 0.76 m/s.

```sim-quiz
id: g-screw
kind: numeric
question: A lead screw advances {lead} mm per turn. What is its linear ratio G, in rad/m?
vary: { lead: { min: 1, max: 12, step: 1 } }
answer_expr: 6.283185 / (lead / 1000)
tolerance: 2%
unit: rad/m
hints:
  - "One turn is 6.283 rad; it moves the nut by one lead."
  - "G = 6.283 / lead, with the lead in metres."
explain: "G = 2π/lead: for 8 mm, 6.283/0.008 = **785 rad/m**, five times the belt's. Each review asks with a different lead."
```

## Force goes the other way

Just as a gear multiplies torque, the ratio multiplies the motor's torque into a pushing force, less what the drive's friction takes (its efficiency η):

```text
F = G·τ·η
```

![A belt on a pulley and a lead screw with its nut, with their ratios G](drives.svg "A belt's G is one over its pulley radius; a lead screw's is one turn over its lead. Speed divides by G, force multiplies by it (times the efficiency).")

**Worked example.** Through the belt (η ≈ 0.95), 0.05 N·m pulls 157 × 0.05 × 0.95 ≈ 7.5 N. Through the 8 mm screw (η ≈ 0.64 when lifting), the same torque pushes 785 × 0.05 × 0.64 ≈ 25 N.

## Can the belt lift it?

Our motor's stall torque is 0.072 N·m. Through the belt that is at most 157 × 0.072 ≈ 11 N of pull.

```sim-quiz
id: predict-belt
kind: predict
scene: belt-lift
question: The motor, at full 12 V, tries to lift the 2 kg carriage (19.6 N) with the belt. What happens?
options:
  - { text: "It lifts it, quickly", feedback: "Quickly, yes, if it could. But 11 N is less than 19.6 N." }
  - { text: "The carriage slides down, turning the motor backwards", correct: true, feedback: "Yes. The load wins, and back-drives the motor against its own torque." }
  - { text: "It holds the carriage still", feedback: "Holding still needs 19.6 N too; the motor can give only 11 N." }
explain: "Stalled, the motor pulls 11 N up while gravity pulls 19.6 N down. The carriage runs down to its lower stop, spinning the motor backwards."
```

```sim-scene
id: belt-lift
system: belt
title: A belt, straight up
caption: At full voltage the motor pulls about 11 N through the belt, less than the 19.6 N weight. The carriage slides down to its stop.
camera: { preset: front, zoom: 1.15 }
run: { duration_s: 0.4, frame_rate: 200 }
script: belt.rhai
plots: [carriage.axis.position, motor.p.current]
show: [forces]
expect:
  - { observe: carriage.axis.velocity, reduce: mean, window: [0.05, 0.12], max: -0.1, why: "The weight (19.6 N) beats the belt's 11 N: the carriage moves down." }
  - { observe: carriage.axis.position, reduce: final, window: [0.0, 0.4], min: -0.052, max: -0.048, why: "It ends on the lower stop at −5 cm." }
```

## The screw

Now the same motor turns a T8 [lead screw](part:screw/screw) with an 8 mm lead: G = 785 rad/m, five times the belt's. Friction in its thread costs more (efficiency about 0.64 lifting), but at stall it can still push 785 × 0.072 × 0.64 ≈ 36 N, well above the load. The next scene lifts with it, and at 0.8 s switches the power off. The companion uses a 2 mm lead (G = 3142 rad/m).

```sim-quiz
id: predict-speed
kind: predict
scene: screw-lift
question: "The 8 mm screw lifts the load steadily with the motor near 430 rad/s. How fast does the carriage rise, in m/s?"
observe: carriage.axis.velocity
reduce: mean
window: [0.25, 0.3]
tolerance: 8%
unit: m/s
hint: v = ω/G, and G = 785 rad/m.
explain: "v = ω/G ≈ 430/785 ≈ **0.55 m/s**. The 2 mm screw, with four times the ratio, needs less torque, so its motor runs faster, but still lifts at only about 0.24 m/s."
```

```sim-scene
id: screw-lift
system: screw
title: Lead screws, 8 mm and 2 mm
caption: "Both lift the 2 kg load to the top stop. At 0.8 s the power goes off: the 8 mm screw (left) runs back down, the 2 mm screw (right, purple) holds."
companion: { label: "2 mm lead", set: { screw.lead: 0.002 }, mode: split }
camera: { preset: front, zoom: 1.15 }
run: { duration_s: 1.4, frame_rate: 120 }
script: screw.rhai
plots: [carriage.axis.position, carriage.axis.velocity]
show: [forces]
expect:
  - { observe: carriage.axis.velocity, reduce: mean, window: [0.25, 0.3], min: 0.5, max: 0.58, why: "v = ω/G with the motor near 430 rad/s: about 0.54 m/s." }
  - { observe: carriage.axis.position, reduce: change, window: [0.9, 1.4], max: -0.08, why: "After power-off the 8 mm screw back-drives: the load runs down, braked by the shorted motor." }
```

```sim-equation
id: speed-now
scene: screw-lift
show: "v = ω / G = ω·lead / 2π"
expr: w * lead / 6.283185
result: { symbol: v, unit: m/s }
terms:
  w: { symbol: ω, unit: rad/s, observe: motor.shaft.speed }
  lead: { symbol: lead, unit: m, param: screw/screw.lead }
holds: { observe: carriage.axis.velocity, window: [0.05, 0.3], tolerance: 2% }
caption: The carriage's speed from the motor's, through the screw's ratio, while it lifts.
```

The 8 mm screw lifted at {{value scene=screw-lift observe=carriage.axis.velocity reduce=mean window=0.25..0.3 | 0.53 m/s}} and ran back down once the power was off. The 2 mm screw was slower, and stayed put.

```sim-quiz
id: why-hold
question: Why does the 2 mm screw hold the load with the power off, while the 8 mm one lets it run down?
options:
  - { text: "Its thread is so shallow that friction holds it, as in a self-locking worm", correct: true, feedback: "Yes: its lead angle (4.5°) is below the thread's friction angle (about 9°); the 8 mm screw's (18°) is above." }
  - { text: "It is stronger", feedback: "Strength is not the question: with the power off neither motor pushes at all." }
  - { text: "The motor brakes it harder", feedback: "Both motors are shorted the same way. The difference is in the thread." }
explain: "A lead screw is a screw, like the worm: it self-locks when its lead angle is below the friction angle. Fine pitches lock; fast pitches back-drive. That also makes fine pitches less efficient: about 0.33 against 0.64."
moment: 1.0
```

## Choosing a drive

The ratio sets speed against force, and the thread sets efficiency against holding:

- belts and racks: low G, fast, efficient (about 0.95), back-drivable. Good for fast horizontal axes; a vertical one needs a brake or a counterweight.
- fast-lead screws: middle G, reasonably efficient, back-drive under load.
- fine-lead screws: high G, strong, slow, often self-locking: they hold a load unpowered, at the cost of efficiency.

## Design the lift

```sim-task
id: lift-design
kind: design
scene: screw-lift
title: A lift that holds without power
goal: "Choose a **lead** so the lift reaches the top stop (12 cm) **before the power goes off at 0.8 s**, and **holds the load** once it is off."
start: { screw.lead: 0.008 }
win:
  - { observe: carriage.axis.position, reduce: mean, window: [0.7, 0.8], min: 0.115, why: "At the top stop before power-off." }
  - { observe: carriage.axis.position, reduce: change, window: [0.9, 1.4], min: -0.002, why: "Holds: less than 2 mm of creep after power-off." }
report:
  - { label: "Position at 0.8 s", observe: carriage.axis.position, reduce: mean, window: [0.7, 0.8], unit: m }
  - { label: "Creep after power-off", observe: carriage.axis.position, reduce: change, window: [0.9, 1.4], unit: m }
  - { label: "Lift speed", observe: carriage.axis.velocity, reduce: max, window: [0.0, 0.7], unit: m/s }
hints:
  - "Which leads self-lock? Compare the lead angle with the friction angle (about 9° for this thread)."
  - "tan λ = lead / (π × 8 mm). A 4 mm lead gives about 9°: right on the edge."
  - "Try 3 mm or 2 mm. Check that it still reaches the top in time."
solution: { screw.lead: 0.002 }
```

## Putting it in your own words

```sim-reflect
id: choose
prompt: You are designing a robot's vertical lift for a 2 kg tray, and a fast horizontal axis for a camera. Which drive would you pick for each, and why? Use the lesson's ideas.
model_answer: "Every linear drive is a ratio G: speed ω/G, force G·τ·η. The lift needs force against gravity and should hold when unpowered, so a fine-lead screw (high G, self-locking) fits: slower, less efficient, but it cannot drop the tray. The camera axis carries no steady load and needs speed, so a belt (low G, efficient) fits; it back-drives freely, which does not matter horizontally. A belt on the lift would need a gearbox for force and a brake or counterweight to hold."
key_points:
  - { idea: "A drive is a ratio G: v = ω/G, F = G·τ·η", cues: ["ratio", "rad/m", "ω/g", "g·τ", "force", "speed"] }
  - { idea: "Lift: high G, self-locking screw", cues: ["fine", "lead screw", "self-lock", "holds", "high g"] }
  - { idea: "Horizontal fast axis: belt", cues: ["belt", "fast", "efficient", "low g"] }
  - { idea: "Back-driving needs a brake or counterweight on vertical axes", cues: ["brake", "counterweight", "back-drive", "backdrive", "drop"] }
```

## Going further

This part is optional.

**Ball screws.** Balls rolling in the thread cut friction to a few percent: efficiency about 0.9, so they back-drive even at fine pitches. Precise and strong, but a vertical axis then needs a brake.

**Belt stretch.** A belt is also a spring: the carriage bounces on it at √(k/m). Long belts on heavy carriages ring at a few tens of hertz, which limits how hard a printer can accelerate.

```sim-component
component: bridge.lead_screw
show: [summary, equations, tradeoffs]
```

## Key ideas

- Every linear drive is a ratio **G** in rad/m: belts and racks 1/r, screws 2π/lead.
- Speed v = ω/G; force F = G·τ·η. A drive can lift a load only if G·τ_stall·η beats its weight.
- Fine-lead screws are strong, slow and often self-locking; belts are fast, efficient and back-drivable.
