---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## recall: Bring it back
[[scroll top]] Before the new idea, let's bring back the last one. <short pause> Write what you remember about torque and inertia, then tick what you had.

## hook: A small motor, a heavy load
[[scroll block:a-small-motor-and-a-heavy-load]] Here's the problem. [[pin part:motor "the motor"]] A small motor, [[pin part:load "the load"]] and a load twenty-five times heavier than its own rotor. [[unpin]]
Between them, a gear pair, five to one.

## trade: Torque up, speed down
[[scroll block:torque-up-speed-down]] [[highlight "So the output turns N times slower"]] For every turn of the output, the pinion turns five times. So the output turns five times slower, <short pause> and pushes five times harder.
[[highlight off]] [[box figure:gear-pair "what passes through"]] Torque goes up, speed goes down, and the power stays the same. The gear trades one for the other. [[unmark]] [[quiz out-torque]]
[[quiz power-through]]

## reflected: The inertia the motor feels
[[scroll block:the-inertia-the-motor-feels]] Now the surprising part. <short pause> How heavy does the load feel, from the motor's side?
[[highlight "divided by N twice"]] It's divided by the ratio, twice. Once because the motor turns faster than the load, and once because the gear multiplies the torque. [[highlight off]]
So through five to one, the load feels twenty-five times lighter. [[quiz reflected-steps]]

## race: Direct or geared
[[scroll quiz:predict-race]] Let's race them. Direct drive, against the five to one gear. Which one swings the load first? [[quiz predict-race]]
[[scene race]] [[scroll scene:race]] [[play]] [[box plot:load.shaft.angle@0..0.03 "first 30 ms"]] Through the gear, the load swings away. <short pause> Direct, it barely moves.
[[wait-scene]] [[unmark]] [[quiz why-gear-won]]

## best: The best ratio
[[scroll block:too-much-of-a-good-thing]] So why not a huge ratio? <short pause> Because the motor also has to spin its own rotor, and from the load's side, the rotor feels N squared times heavier.
[[highlight "The sweet spot is where"]] The sweet spot is where the load, as the motor feels it, equals the rotor. [[highlight off]] [[quiz best-ratio]]

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain what a big gearbox gives a robot joint, and what it costs.
[[scroll block:key-ideas]] [[highlight "multiplies torque by N"]] Torque times N, speed divided by N. [[highlight "Reflected inertia"]] And the load, divided by N squared. [[highlight off]]
