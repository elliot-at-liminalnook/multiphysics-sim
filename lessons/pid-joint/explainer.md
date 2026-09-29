---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: Go to one radian
[[scroll top]] Tell a robot arm to go to one radian. <short pause> It swings past, comes back, and settles a little low. Let's fix that, one term at a time.

## p: Proportional
[[scroll block:p-push-in-proportion-to-the-error]] [[highlight "pushes harder the further the joint is from its target"]] The simplest controller pushes harder the further the joint is from its target. [[highlight off]] [[quiz command]]

## p-scene: P alone
[[scroll block:p-alone-overshoot-then-a-sag]] P alone has two problems. <short pause> Momentum carries the arm past the target. And holding it up against gravity needs a steady error. [[quiz predict-p]]
[[scene p-only]] [[scroll scene:p-only]] [[play-until 0.45]] Full drive toward the target, [[play-until 0.7]] and past it. [[play]] [[wait-scene]] Then it settles, a little short. [[quiz double-kp]]

## d: Derivative
[[scroll block:d-brake-as-you-approach]] [[highlight "It acts like a damper"]] The derivative term pushes against the speed. It acts like a damper, braking the arm as it approaches. [[highlight off]] [[quiz predict-d]]
[[scene add-d]] [[scroll scene:add-d]] [[play]] [[wait-scene]] Almost no overshoot. <short pause> But still that sag.

## i: Integral
[[scroll block:i-remove-the-last-error]] [[highlight "adds up the error over time"]] The integral term adds up the error over time, and keeps raising the command until the error is gone. [[highlight off]] [[quiz why-i]]
[[scroll scene:add-i]] Now you tune it. P first, then D, then a little I.

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain P, I and D to someone new to control.
[[scroll block:key-ideas]] [[highlight "P"]] P is a spring. [[highlight "D"]] D is a damper. [[highlight "I"]] I is a memory. [[highlight off]]
