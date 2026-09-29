---
title: "Model meets measurement: the knee servo"
summary: The robot's knee servo was measured on the bench. Which of its model's numbers were measured, how well does the model match the data, and what does the mismatch tell us?
order: 4
category: sensing-and-control
minutes: 20
requires: [motor-torque-speed]
teaches: [model-fidelity]
needs: [voltage-budget, torque-speed-line]
systems:
  knee: ../../examples/actuators/hx30hm/accepted/hx30hm-knee-bench.system.json
authors: [Systems builder]
---
# The knee servo, measured

**By the end of this lesson you will be able to:**

- say which numbers in a model were measured, derived or estimated;
- read a model-against-measurement plot, and tell a scale error from an offset;
- explain why agreement with the data a model was fitted to proves less than it seems.

The robot's knee is driven by a small hobby servo: a DC motor behind a 200:1 gearbox. On 21 September its steady speed was measured on the bench, with the leg hanging free, at four drive levels. The lesson's model of that bench is the same kind of motor you met in the first lesson, plus a gearbox, friction and the leg.

## What the model is made of

Every number in a model has a history. Some were **measured** on this hardware, some derived from other numbers, and some **estimated** from a datasheet or a similar part. The model keeps that history with each value.

For this servo, the motor constant k = {{param servo/motor.back_emf_constant | 0.0121 V·s/rad}} and the output friction {{param servo/friction.torque | 0.136 N·m}} were fitted to the bench measurements. The gear ratio {{param servo/gear.ratio | 200}} and the winding resistance {{param servo/motor.resistance | 3.7 Ω}} are estimates from the servo family.

```sim-quiz
id: which-measured
question: Which of these values of the knee model came from measurements on this servo?
options:
  - { text: "The gear ratio", feedback: "The ratio is an estimate from the servo family; nobody opened this servo to count teeth." }
  - { text: "The motor constant k and the output friction", correct: true, feedback: "Yes. Both were fitted to the bench's steady speeds." }
  - { text: "The leg's inertia", feedback: "That is an order-of-magnitude estimate for the printed leg." }
explain: The card below lists every parameter with its origin. Only k and the friction were measured; everything else is an estimate or a derived value, and should be treated as less certain.
concepts: [model-fidelity]
```

## Model beside measurement

Below, each measured point (orange) sits beside the model's simulation of the same drive (teal). Before you read the numbers, look at the pattern of the gaps: are they random, or do they lean one way?

```sim-measured
id: knee-steady
system: knee
data: ../../examples/actuators/hx30hm/accepted/knee-steady-speeds.json
title: Steady knee speed against drive
caption: Two runs at each drive. The model is simulated once per drive level on the detailed model; the gaps are simulated minus measured.
x: { field: duty, label: Duty, unit: "" }
y: { field: speed_rad_s, label: Output speed, unit: rad/s, observe: leg.shaft.speed, reduce: mean, window: [0.6, 1.0] }
set: { supply.voltage: duty * supply_v }
run: { duration_s: 1.0, frame_rate: 100 }
max_rms: 0.2
max_gap: 0.25
```

The model's speeds are symmetric: forward and backward drives give the same size of speed. The knee's are not: at 15 % drive it turned at {{data knee-steady duty=0.15 | 0.45 rad/s}} forwards and {{data knee-steady duty=-0.15 | -0.75 rad/s}} backwards.

```sim-quiz
id: read-the-gap
question: Every gap is positive, by about 0.15–0.22 rad/s, in both directions. What does that pattern point to?
options:
  - { text: "A torque that always pushes the same way, such as gravity on the hanging leg", correct: true, feedback: "Yes. A constant torque shifts every speed the same way; the model's gravity torque is set to zero." }
  - { text: "The motor constant k is wrong", feedback: "A wrong k would scale the speeds, making the gaps grow with the drive and change sign with direction.", remedy: scale-vs-offset }
  - { text: "Measurement noise", feedback: "Noise would scatter the gaps both ways. These all lean the same way.", remedy: scale-vs-offset }
explain: The fitted model is right on average (0.6 rad/s is the mean of 0.45 and 0.75) but misses a one-way torque. On the suspended leg that is gravity, which the model sets to zero at mid-range.
concepts: [model-fidelity]
```

```sim-remedy
id: scale-vs-offset
misconception: "Any mismatch means a wrong constant"
body: "Look at how a gap behaves. A wrong **scale** (k, a gear ratio) changes the gap in proportion to the speed and flips its sign when the direction flips. An **offset** (a constant torque, a bias) moves every point the same way. Random **noise** scatters gaps in both directions. Here every gap is positive and roughly the same size: an offset."
then: read-the-gap
```

The model was **fitted to this very data**: k and the friction were chosen to match these points. So the close average agreement is partly by construction. It shows the fit worked, not that the model predicts anything new. A fair test uses measurements the fit never saw.

## Your own bench

If you have the leg on its fixture and the calibration server running, you can measure one point yourself and see where it falls. Without a bench the card still shows what the model and the recorded measurements say.

```sim-lab
id: knee-quarter
title: One steady speed at 15 % drive
joint: knee
test: { duty: 0.15, seconds: 1.0 }
compare: knee-steady
predict: "Before running it: what steady speed do you expect at 15 % drive? Use the plot above."
unit: rad/s
notes: The server first backs the knee to the far end of its taught window, then drives it at 15 % for up to a second, stopping early rather than run into the window's end.
```

## In your own words

```sim-reflect
id: why-fit-not-proof
prompt: A teammate says "the model matches the bench data within 0.2 rad/s, so it is validated". In two or three sentences, say what that agreement does and does not show.
model_answer: "The model's k and friction were fitted to these same points, so matching them on average is expected: it shows the fit worked, not that the model predicts well. The gaps also lean one way, which points to a missing one-way torque (gravity on the hanging leg). To validate the model we need measurements it was not fitted to, ideally including the gravity load."
key_points:
  - { idea: "It was fitted to the same data", cues: ["fitted", "same data", "same points", "tuned to", "chosen to match"] }
  - { idea: "Agreement shows the fit, not prediction", cues: ["not predict", "by construction", "expected to match", "not a test", "not validated"] }
  - { idea: "The one-way gap points to a missing torque (gravity)", cues: ["gravity", "offset", "bias", "one way", "same direction"] }
  - { idea: "Validation needs new measurements", cues: ["new data", "new measurements", "other data", "not fitted", "unseen", "held out"] }
```

## Key ideas

- Every model value is **measured**, **derived** or **estimated**; the model records which, and the lesson shows it.
- Read the pattern of the gaps: a scale error grows with speed and flips with direction, an offset leans one way, noise scatters.
- Agreement with the data a model was fitted to proves the fit, not the model. Test against data it has not seen.
