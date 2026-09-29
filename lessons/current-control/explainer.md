---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: Asking for a torque
[[scroll top]] A walking robot usually wants to decide the torque at each joint: <short pause> push this hard, land this softly. And a motor's torque is k times the current.

## fade: A voltage command fades
[[scroll block:a-voltage-command-fades]] Set a motor's voltage, and its torque is only steady while it stands still. <short pause> As it speeds up, the back-EMF takes more of the voltage, and the torque fades. [[quiz fade]]

## loop: A loop that holds the current
[[scroll block:a-loop-that-holds-the-current]] [[box figure:current-loop "the loop"]] A current loop measures the current and adjusts the voltage. [[unmark]]
[[highlight "raises the voltage"]] Whenever the back-EMF grows and the current starts to fall, it raises the voltage. [[highlight off]] [[quiz loop-sees]]

## watch: Holding two amps
[[scroll quiz:predict-hold]] The loop holds two amps while the wheel spins up. What will the current be at point four seconds? [[quiz predict-hold]]
[[scene spin-up]] [[scroll scene:spin-up]] [[play-until 0.03]] The current rises to two amps in a couple of milliseconds. [[play-until 0.6]] And stays there, while the speed climbs in a straight line. <short pause> The fixed voltage fades from the start.
[[play]] [[wait-scene]] Until the command hits a hundred percent, and the supply runs out. [[quiz straight-line]]

## limit: Where it runs out
[[scroll block:where-it-runs-out]] [[highlight "the loop can no longer hold the current"]] Once the back-EMF has eaten the rest of the supply, the loop can no longer hold the current. [[highlight off]] [[quiz omega-max]]

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain why a robot's joints should take torque commands through a current loop.
[[scroll block:key-ideas]] [[highlight "command"]] To command torque, command current. [[highlight off]]
