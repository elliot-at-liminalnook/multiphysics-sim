---
title: Resonance — why a robot shakes at one particular speed
summary: A payload on a springy mount is tapped, then shaken at a rising frequency. At one frequency, √(k/J), the swing grows eight times larger than a steady push gives. How to predict it, and how to keep a robot away from it.
order: 14
category: mechanisms
minutes: 20
requires: [series-elastic]
systems:
  mount: mount.system.json
authors: [Systems builder]
teaches: [natural-frequency, resonance, damping-ratio]
needs: [spring-torque, rotational-inertia]
---
# A mount that rings

**By the end of this lesson you will be able to:**

- calculate a spring–inertia system's natural frequency;
- predict how much larger the motion gets when it is shaken at that frequency;
- say how a designer keeps a robot away from resonance.

A camera on a walking robot shakes badly at one walking pace and hardly at all at others. A 3D printer's belt chatters at one speed. Both are resonance. Here a [payload](part:mount/payload) sits on a springy [mount](part:mount/mount) with a little [damping](part:mount/damper), and a [shaker](part:mount/shaker) pushes it back and forth.

## The natural frequency

Twist the payload and let go: the spring pulls it back, it overshoots, and it swings back and forth by itself. How fast depends on only two things, the spring's stiffness k and the payload's inertia J. A stiffer spring pulls harder, so it swings faster; a heavier payload swings slower:

```text
ω_n = √(k / J)          f_n = ω_n / 2π
```

**Worked example.** Our mount has k = {{param mount.stiffness | 4 N·m/rad}} and J = {{param payload.inertia | 0.001 kg·m²}}: ω_n = √4000 ≈ 63 rad/s, and f_n = 63 / 6.28 ≈ 10 Hz. Left alone, it swings ten times a second.

```sim-quiz
id: fn
kind: numeric
question: Our payload (J = 0.001 kg·m²) sits on a mount of stiffness {k} N·m/rad. What is its natural frequency, in hertz?
vary: { k: { min: 1, max: 40, step: 1 } }
given: { J: payload.inertia }
answer_expr: sqrt(k / J) / 6.283185
tolerance: 3%
unit: Hz
hints:
  - "First ω_n = √(k/J), in rad/s."
  - "Then divide by 2π to get cycles per second."
  - "√(k / 0.001), then ÷ 6.28."
explain: "f_n = √(k/J)/2π. For 4 N·m/rad: √4000/6.28 ≈ **10.1 Hz**. Four times stiffer doubles it. Each review asks with a different stiffness."
concepts: [natural-frequency]
```

## A tap

The first scene gives the payload one sharp 10 ms push and leaves it alone.

```sim-quiz
id: predict-tap
kind: predict
scene: tap
question: After the tap, what will the payload do?
options:
  - { text: "Swing back and forth about ten times a second, each swing a little smaller", correct: true, feedback: "Yes: it rings at its natural frequency while the damping slowly takes the energy out." }
  - { text: "Swing once and stop", feedback: "That would need heavy damping. This mount is lightly damped, like most metal structures." }
  - { text: "Move over and stay there", feedback: "The spring always pulls it back to the middle." }
explain: "It rings at about 10 Hz. Each swing is about 27 % smaller than the last: that is set by the damping."
```

```sim-scene
id: tap
system: mount
title: One tap
caption: A 10 ms push, then nothing. The payload rings at its natural frequency, and the ringing dies away.
set: { shaker.amplitude: 0, shaker.step: 0.2, shaker.start: 0.1, shaker.duration: 0.01, shaker.edge: 0.001 }
camera: { preset: iso, zoom: 1.5 }
run: { duration_s: 1.5, frame_rate: 240 }
script: tap.rhai
plots: [payload.shaft.angle]
show: [trails]
expect:
  - { observe: payload.shaft.angle, reduce: max, window: [0.1, 0.2], min: 0.025, max: 0.033, why: "The tap's impulse (0.2 N·m × 10 ms) gives a first swing of about 0.029 rad." }
  - { observe: payload.shaft.angle, reduce: max, window: [1.0, 1.1], max: 0.003, why: "The ringing decays as e^(−ζ·ω_n·t) = e^(−3.2 t): after 0.9 s, under a tenth." }
```

## How fast it dies away

The ringing fades because the [damper](part:mount/damper) turns a little of the motion into heat on every swing. How much, compared with the spring and payload, is captured by one number, the **damping ratio**:

```text
ζ = c / (2·√(k·J))
```

**Worked example.** c = {{param damper.damping | 0.0063 N·m·s/rad}}: ζ = 0.0063 / (2 × √0.004) = 0.0063 / 0.126 = 0.05. Values like 0.02–0.05 are typical of bolted metal structures; ζ = 1 means it returns without swinging at all.

```sim-quiz
id: zeta
kind: numeric
question: A stickier mount material gives c = {c} N·m·s/rad, with the same k = 4 N·m/rad and J = 0.001 kg·m². What is its damping ratio?
vary: { c: { min: 0.005, max: 0.1, step: 0.005 } }
given: { k: mount.stiffness, J: payload.inertia }
answer_expr: c / (2 * sqrt(k * J))
tolerance: 3%
unit: ""
hint: "ζ = c / (2·√(k·J)), and 2·√(4 × 0.001) = 0.126."
explain: "ζ = c/0.126. For c = 0.025: **0.2**. Four times the damping of our mount, so its ringing dies four times faster."
concepts: [damping-ratio]
```

## Shaken at every frequency

Now the shaker pushes with a gentle ±0.05 N·m sine, whose frequency climbs steadily: 2 Hz at the start, rising 4 Hz every second, so 10 Hz at 2 s and 18 Hz at 4 s. Pushed that hard steadily, the mount would bend by only τ/k = 0.0125 rad.

```sim-quiz
id: predict-sweep
kind: predict
scene: sweep
question: When will the payload swing the most?
options:
  - { text: "At the start, while the shaking is slow", feedback: "At low frequency the payload just follows the push: about 0.0125 rad." }
  - { text: "Around 2 s, when the shaking passes 10 Hz", correct: true, feedback: "Yes. Each push then arrives just in time to add to the swing." }
  - { text: "At the end, when the shaking is fastest", feedback: "Far above the natural frequency the payload cannot keep up, and barely moves." }
explain: "The swing peaks at about {{value scene=sweep observe=payload.shaft.angle reduce=max window=0..4 | 0.097 rad}}, near 2.1 s: about eight times the steady bend."
```

```sim-scene
id: sweep
system: mount
title: A frequency sweep
caption: "The shaking frequency climbs from 2 Hz by 4 Hz each second. Near 10 Hz (2 s) the swing grows about eight times; the purple ghost has four times the damping."
companion: { label: "4× damping (ζ = 0.2)", set: { damper.damping: 0.025 }, mode: ghost }
camera: { preset: iso, zoom: 1.5 }
run: { duration_s: 4.0, frame_rate: 240 }
script: sweep.rhai
plots: [payload.shaft.angle]
show: [trails]
sliders:
  - { parameter: mount.stiffness, label: "Mount stiffness", min: 1, max: 60, step: 1, unit: "N·m/rad" }
  - { parameter: damper.damping, label: "Mount damping", min: 0.001, max: 0.08, step: 0.001, unit: "N·m·s/rad" }
hints:
  - "Stiffen the mount to 16 N·m/rad: the peak moves to 20 Hz, later in the sweep."
  - "Raise the damping: the peak shrinks and widens."
challenge:
  goal: "Change the **mount** so the payload never swings more than **0.03 rad** during the whole sweep."
  hint: "Two ways: damp it (the peak is about 1/(2ζ) times the steady bend), or stiffen it so its natural frequency is above anything the shaker reaches."
  win:
    - { observe: payload.shaft.angle, reduce: peak, window: [0.0, 4.0], max: 0.03, why: "The swing stays under 0.03 rad at every frequency." }
expect:
  - { observe: payload.shaft.angle, reduce: max, window: [1.9, 2.5], min: 0.08, max: 0.11, why: "Near f_n = 10 Hz the swing builds to about 0.097 rad." }
  - { observe: payload.shaft.angle, reduce: max, window: [0.0, 0.75], max: 0.02, why: "Well below f_n it follows the push: about τ/k = 0.0125 rad." }
  - { observe: payload.shaft.angle, reduce: max, window: [3.5, 4.0], max: 0.01, why: "Well above f_n the payload barely moves." }
```

```sim-equation
id: fn-live
scene: sweep
show: "f_n = √(k/J) / 2π"
expr: sqrt(k / J) / 6.283185
result: { symbol: f_n, unit: Hz }
terms:
  k: { symbol: k, unit: N·m/rad, param: mount.stiffness }
  J: { symbol: J, unit: kg·m², param: payload.inertia }
caption: The natural frequency from the mount's values. Move the stiffness slider and watch where the peak lands in the sweep (frequency = 2 + 4·t Hz).
```

## How tall the peak gets

At the natural frequency the push and the swing stay in step: every push adds a little energy, and the swing grows until the damper takes out as much energy per cycle as the shaker puts in. With light damping that happens only when the swing is large: about 1/(2ζ) times the steady bend.

![Amplification against shaking frequency for two damping ratios](response.svg "At the natural frequency a lightly damped mount (ζ = 0.05) swings about ten times more than a steady push would bend it; with ζ = 0.2, about 2.5 times.")

```sim-quiz
id: amplification
question: "Our mount has ζ = 0.05. Held at exactly 10 Hz long enough, about how much larger than the steady bend (0.0125 rad) would the swing get?"
options:
  - { text: "About 10 times: 0.125 rad", correct: true, feedback: "Yes, 1/(2ζ) = 10. The sweep passed through too quickly to get all the way (about 8×)." }
  - { text: "About 2 times", feedback: "That is what ζ = 0.25 would give. Our mount is much more lightly damped." }
  - { text: "Without limit: it keeps growing", feedback: "Only with no damping at all. The damper sets the ceiling." }
explain: "Peak amplification ≈ 1/(2ζ) = 1/(2 × 0.05) = **10**, so about 0.125 rad. The sweep reached 0.097 rad because it moved on before the swing had fully built up."
concepts: [resonance, damping-ratio]
```

## Keeping a robot out of resonance

Three levers, all visible in the formula and the figure:

- Move the natural frequency away from anything that shakes the robot: stiffer structures and lighter payloads raise it. Aim for at least 1.5–2 times any excitation.
- Add damping: rubber mounts, damped couplings, or a controller that brakes the motion.
- Stop shaking at that frequency: change the gait timing, the motor speed or the step rate.

```sim-quiz
id: gait
question: A robot's camera mast rings at 3 Hz, and the robot walks at 3 steps per second. Which change fixes the shaking best?
options:
  - { text: "Stiffen the mast so it rings above 6 Hz", correct: true, feedback: "Yes: the walking pushes then fall well below its natural frequency." }
  - { text: "Walk at 2.8 steps per second", feedback: "Still close enough to 3 Hz to excite it strongly, especially with light damping." }
  - { text: "Make the mast more flexible", feedback: "That lowers its natural frequency, but toward the walking's second harmonic (6 Hz) and with larger swings." }
explain: "Moving f_n well above the excitation (here to twice it or more) makes the mast follow the steps almost rigidly. Damping helps too, but moving away from the frequency helps most."
```

## Putting it in your own words

```sim-reflect
id: why-shake
prompt: Explain to a colleague, in a few sentences, why their robot arm shakes violently when it moves at one particular speed but not at others, and what they could do about it.
model_answer: "The arm and its compliant parts (gearbox, belt, structure) form a spring–inertia system with a natural frequency √(k/J)/2π. Moving at that particular speed shakes it at that frequency, so each push arrives in step with the swing and adds energy, and the motion grows until the small damping balances it, many times larger than the same push would give at other frequencies (about 1/(2ζ)). Fixes: make the structure stiffer or the load lighter to move the natural frequency well above the excitation, add damping, or avoid moving at that speed."
key_points:
  - { idea: "There is a natural frequency √(k/J)", cues: ["natural frequency", "√(k/j)", "sqrt", "k/j", "rings"] }
  - { idea: "Pushes in step with the swing keep adding energy", cues: ["in step", "in phase", "adds energy", "builds", "same frequency", "matches"] }
  - { idea: "Light damping lets it grow large (≈ 1/(2ζ))", cues: ["damping", "1/(2ζ)", "ζ", "larger", "amplif"] }
  - { idea: "Fix: stiffen, lighten, damp, or avoid that speed", cues: ["stiffer", "stiffen", "lighter", "damp", "avoid", "change speed"] }
```

## Going further

This part is optional.

**Many modes.** A real robot has many natural frequencies, one per way it can flex. The lowest usually matters most, because it is the easiest to excite and the biggest.

**Resonance in control.** A controller whose bandwidth reaches a structure's natural frequency can pump energy into it: the joint then "sings". Controllers are usually kept a factor of 3–5 below the lowest mode, or given a notch filter at that frequency.

```sim-compare
id: stiffness-sweep
system: mount
study: stiffness
title: Largest swing during the sweep, for three mount stiffnesses
caption: "The stiffer mount's peak moves to a higher frequency and its steady bend shrinks; the swing is largest where resonance meets a slow enough sweep."
```

```sim-component
component: rotational.damper
show: [summary, equations, tradeoffs]
```

## Key ideas

- A spring and an inertia swing at their **natural frequency** ω_n = √(k/J).
- Shaken at that frequency, a lightly damped system's motion builds to about 1/(2ζ) times the steady bend: **resonance**.
- The **damping ratio** ζ = c/(2·√(k·J)) sets how fast ringing dies and how tall the peak is.
- Keep excitations well away from f_n: stiffen, lighten, damp, or change the timing.
