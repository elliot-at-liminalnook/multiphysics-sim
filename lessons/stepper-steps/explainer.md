---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: The layer shift
[[scroll top]] A 3D printer suddenly prints the rest of an object shifted sideways. <short pause> Its motors kept stepping, and nothing noticed. Let's see how.

## spring: A magnetic spring with teeth
[[scroll block:a-magnetic-spring-with-teeth]] [[highlight "pulled toward that target like a stiff spring"]] A stepper's rotor is pulled toward the driver's target like a stiff spring. [[highlight off]]
[[box figure:stepper-torque "the pull"]] The pull peaks one step behind. <short pause> Two steps behind, it falls to zero, and further back it reverses. [[unmark]] [[quiz lag]]

## accel: Accelerating costs torque
[[scroll block:accelerating-costs-torque]] To follow a target that speeds up, the rotor needs torque, J times alpha. <short pause> And the stepper can give at most its holding torque. [[quiz amax]]

## move: Too fast a start
[[scroll quiz:predict-lost]] Now we'll ask for more acceleration than it can give. [[quiz predict-lost]]
[[scene move]] [[scroll scene:move]] [[play-until 0.066]] The rotor tries to follow. [[play-until 0.1]] Two steps behind, it slips. [[play]] [[wait-scene]] And now it only buzzes, while the target races away. The gentle move, on the right, follows all the way.
[[quiz why-buzz]]

## ramp: Ramp it
[[scroll block:ramp-it-with-margin]] So: ramp every move gently, with margin. [[scroll block:design-the-move]] Now try it: the fastest move that doesn't lose a step.

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain why a printer's layers can shift.
[[scroll block:key-ideas]] [[highlight "holding torque"]] A stepper pulls hardest one step behind. [[highlight "Ramp every move"]] Ramp every move, with margin. [[highlight off]]
