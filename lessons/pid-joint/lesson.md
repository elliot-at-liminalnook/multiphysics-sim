---
title: PID — making a joint go where it is told
summary: A gearmotor lifts an arm to 1 rad under a PID controller. Proportional action alone overshoots and then stops short under gravity; derivative action tames the overshoot; integral action removes the last error. Each term, what it does, and how to tune them in order.
order: 26
category: sensing-and-control
minutes: 25
requires: [gravity-torque]
systems:
  joint: joint.system.json
authors: [Systems builder]
teaches: [pid-control]
needs: [holding-torque, gravity-torque]
---
# Go to one radian

**By the end of this lesson you will be able to:**

- say what the P, I and D terms of a controller each do to a joint;
- predict a proportional controller's overshoot and steady error, and why gravity causes the error;
- tune a joint in the usual order: P, then D, then I.

A robot arm is told to go to 1 rad. It swings past, comes back, and settles a little low. Here a [gearmotor](part:joint/motor) drives an [arm](part:joint/arm) through a [30:1 gearbox](part:joint/gearbox); an [encoder](part:joint/encoder) measures its angle, and a [PID controller](part:joint/pid) turns the error into a drive command for the [driver](part:joint/driver). At 0.2 s the [target](part:joint/target) steps from 0 to 1 rad.

## P: push in proportion to the error

The simplest controller pushes harder the further the joint is from its target. The **error** is e = target − angle, and the command is proportional to it:

```text
u = kp·e          (u from −1 to 1, clipped)
```

**Worked example.** With kp = {{param pid.kp | 4}} per radian, a 1 rad error asks for u = 4, which clips to full drive. At 0.1 rad of error: 0.4, or 40 % of the supply.

```sim-quiz
id: command
kind: numeric
question: With kp = {kp} per radian, what command does an error of 0.1 rad give? (0 to 1)
vary: { kp: { min: 1, max: 9, step: 0.5 } }
answer_expr: kp * 0.1
tolerance: 2%
unit: ""
hint: u = kp·e.
explain: "u = kp·e: with kp = 4, **0.4**. Large errors saturate at full drive; only near the target is the command proportional."
```

## P alone: overshoot, then a sag

Proportional action has two problems. Arriving at full speed, the arm's momentum carries it past the target before the reversed command can stop it: overshoot. And once still, it must hold the arm up against gravity, which takes a steady command, and a P controller only gives a steady command when there is a steady error. So it settles a little short, exactly like the servo that sagged under a leg.

```sim-quiz
id: predict-p
kind: predict
scene: p-only
question: "Under P control alone (kp = 4), where will the arm settle after its target steps to 1 rad?"
options:
  - { text: "Exactly at 1 rad", feedback: "Holding the arm up against gravity needs a steady command, which a P controller only gives with a steady error." }
  - { text: "A little below 1 rad, after overshooting", correct: true, feedback: "Yes: it overshoots to about 1.29 rad, then settles about 0.04 rad short." }
  - { text: "Above 1 rad", feedback: "Gravity pulls it down, so the error that holds it is below the target." }
explain: "It overshoots to about 1.29 rad and settles at {{value scene=p-only observe=arm.shaft.angle reduce=mean window=1.8..2 | 0.962 rad}}: the error that makes just enough command to hold the arm up."
```

```sim-scene
id: p-only
system: joint
title: Proportional control alone
caption: "The target steps to 1 rad at 0.2 s. P alone (kp = 4, solid) overshoots and settles short; the ghost doubles kp: more overshoot, smaller sag."
companion: { label: "kp = 8", set: { pid.kp: 8 }, mode: ghost }
camera: { preset: front, zoom: 1.4 }
run: { duration_s: 2.0, frame_rate: 240 }
script: step.rhai
plots: [arm.shaft.angle, pid.command]
show: [forces, trails]
expect:
  - { observe: arm.shaft.angle, reduce: max, window: [0.2, 2.0], min: 1.25, max: 1.33, why: "P alone overshoots to about 1.29 rad." }
  - { observe: arm.shaft.angle, reduce: mean, window: [1.8, 2.0], min: 0.95, max: 0.975, why: "It settles about 0.04 rad short: the error kp needs to hold the arm against gravity." }
```

```sim-quiz
id: double-kp
question: "Doubling kp (the ghost) roughly halves the sag. Why not just make kp enormous?"
options:
  - { text: "The overshoot grows and the joint rings, and in a real loop with delay it becomes unstable", correct: true, feedback: "Yes. A stiffer spring on the same inertia swings harder; with any delay it can oscillate on its own." }
  - { text: "The motor would draw no current", feedback: "It would draw more, not less, and saturate at full drive for longer." }
  - { text: "The sag would grow", feedback: "The sag shrinks as kp grows: it is τ divided by the joint's stiffness." }
explain: "High kp is a stiff spring: small sag, but violent overshoot and ringing, and with the delays of a real loop (next lesson) it goes unstable. Other terms fix the problems more gracefully."
moment: 0.5
concepts: [pid-control]
```

## D: brake as you approach

The derivative term pushes against the error's rate of change, which near the target means against the arm's speed. It acts like a damper: it lets the arm go fast when far away, and brakes it as it approaches:

```text
u = kp·e + kd·de/dt
```

**Worked example.** Approaching at 10 rad/s with kd = 0.1 s/rad, the D term subtracts 1.0 from the command: full reverse drive, even with the arm still short of the target.

```sim-quiz
id: predict-d
kind: predict
scene: add-d
question: With kd = 0.1 added, how far will the arm overshoot 1 rad?
options:
  - { text: "About as much as before (0.29 rad)", feedback: "The D term brakes it on the way in." }
  - { text: "Hardly at all", correct: true, feedback: "Yes: it peaks just above 1.0." }
  - { text: "It never reaches the target at all", feedback: "It still gets close; what remains is the gravity sag." }
explain: "The arm peaks at about {{value scene=add-d observe=arm.shaft.angle reduce=max window=0.2..2 | 1.011 rad}}. But it still settles short: D does nothing about a steady error."
```

```sim-scene
id: add-d
system: joint
title: Adding derivative action
caption: "PD control (kp = 4, kd = 0.1, solid): almost no overshoot, but still a sag. The ghost is P alone."
set: { pid.kd: 0.1 }
companion: { label: "P alone", set: { pid.kd: 0 }, mode: ghost }
camera: { preset: front, zoom: 1.4 }
run: { duration_s: 2.0, frame_rate: 240 }
script: step.rhai
plots: [arm.shaft.angle, pid.command]
show: [forces, trails]
expect:
  - { observe: arm.shaft.angle, reduce: max, window: [0.2, 2.0], max: 1.03, why: "The D term brakes the approach: overshoot about 1 %." }
  - { observe: arm.shaft.angle, reduce: mean, window: [1.8, 2.0], min: 0.955, max: 0.975, why: "Still short: D only acts while the error changes." }
```

## I: remove the last error

The integral term adds up the error over time. While any error remains, it keeps growing the command, until the arm is exactly on target; then it stops growing and holds that command, which is exactly the command gravity needs. The steady error vanishes:

```text
u = kp·e + ki·∫e dt + kd·de/dt
```

**Worked example.** With ki = 4 per rad·s, a steady 0.04 rad error adds 0.16 to the command every second. Within a fraction of a second it has supplied the 0.2 or so of command that holding the arm needs.

```sim-quiz
id: why-i
question: "Once the arm is exactly on target and still, the error is zero. How can the I term still hold it up?"
options:
  - { text: "Its accumulated sum stays where it got to, still supplying the holding command", correct: true, feedback: "Yes. The integral stops changing when the error is zero, but keeps its value." }
  - { text: "It cannot: the arm will sag again", feedback: "That is what P alone does. The integral remembers." }
  - { text: "The D term holds it", feedback: "D is zero when nothing moves." }
explain: "The integral's value is the memory of past error. At the target it holds whatever command was needed to get there: here, the gravity-holding command."
concepts: [pid-control]
```

```sim-scene
id: add-i
system: joint
title: Full PID
caption: "Tune it yourself. Start from P alone (kp = 4); the target steps to 1 rad at 0.2 s."
camera: { preset: front, zoom: 1.4 }
run: { duration_s: 2.0, frame_rate: 240 }
script: step.rhai
plots: [arm.shaft.angle, pid.command]
show: [forces, trails]
sliders:
  - { parameter: pid.kp, label: "kp", min: 0.5, max: 16, step: 0.5, unit: "1/rad" }
  - { parameter: pid.kd, label: "kd", min: 0, max: 0.4, step: 0.01, unit: "s/rad" }
  - { parameter: pid.ki, label: "ki", min: 0, max: 20, step: 0.5, unit: "1/(rad·s)" }
hints:
  - "The usual order: raise kp until it overshoots, add kd until the overshoot is gone, then add a little ki for the last error."
  - "Too much ki overshoots slowly and hunts; too much kd makes the joint sluggish."
challenge:
  goal: "Tune the joint to overshoot by **no more than 0.05 rad** and sit **within 0.01 rad of 1 rad** between 1.4 and 1.6 s."
  hint: "P for speed, D against the overshoot, I for the gravity sag. Try kp 4, then kd around 0.15, then ki around 4."
  win:
    - { observe: arm.shaft.angle, reduce: max, window: [0.2, 2.0], max: 1.05, why: "Overshoot at most 0.05 rad." }
    - { observe: arm.shaft.angle, reduce: mean, window: [1.4, 1.6], min: 0.99, max: 1.01, why: "On target within 0.01 rad by 1.4 s." }
expect:
  - { observe: arm.shaft.angle, reduce: mean, window: [1.8, 2.0], min: 0.95, max: 0.975, why: "Starting from P alone: it settles short, as before." }
```

## Putting it in your own words

```sim-reflect
id: explain-pid
prompt: Explain to a teammate who is new to control, in a few sentences, what each of P, I and D does for a robot joint, and in what order to tune them.
model_answer: "P pushes in proportion to the error, like a spring pulling the joint toward its target: more kp means stiffer and faster, but with more overshoot, and against a steady load such as gravity it leaves a steady error, because it only pushes when there is an error. D pushes against the error's rate of change, like a damper, braking the joint as it approaches so it does not overshoot. I adds up the error over time and keeps increasing the command until the error is gone, then holds that command: it removes the steady error from gravity or friction, but too much makes the joint slow to settle and hunt. Tune kp first until it overshoots, add kd to remove the overshoot, then a little ki for the last error."
key_points:
  - { idea: "P: a spring toward the target (faster, overshoots)", cues: ["proportional", "spring", "stiff", "overshoot", "error"] }
  - { idea: "P alone leaves a steady error under gravity", cues: ["steady error", "sag", "gravity", "short", "offset"] }
  - { idea: "D: a damper that brakes the approach", cues: ["derivative", "damp", "brake", "rate", "speed"] }
  - { idea: "I: accumulates error, removes the steady error", cues: ["integral", "accumulat", "sum", "remove", "memory"] }
  - { idea: "Order: P, then D, then I", cues: ["first", "then", "order", "kp", "kd", "ki"] }
```

## Going further

This part is optional.

**Integral wind-up.** While the command is saturated (the early part of the step), the error is large and the integral keeps growing, then overshoots once the joint arrives. Real controllers stop integrating while saturated (anti-wind-up). This one does not, which is why large ki overshoots here.

**Feed-forward.** If you know the gravity torque (m·g·r·sin φ), you can add its command directly instead of waiting for the integral to find it. The integral then only mops up what the model missed.

```sim-component
component: part.pid_angle
show: [summary, equations, tradeoffs]
```

## Key ideas

- **P** pulls toward the target like a spring: faster with more kp, but it overshoots and leaves a steady error against loads.
- **D** damps: it brakes the approach and removes overshoot, and does nothing when still.
- **I** accumulates the error and removes the steady error, holding whatever command the load needs.
- Tune in order: kp, then kd, then a little ki.
