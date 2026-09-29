---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: Is it enough?
[[scroll top]] A servo's datasheet says twelve kilogram centimetres. <short pause> Is that enough to lift a leg? Let's find out.
[[pin part:leg "the leg"]] Our leg weighs point four kilograms, its centre of mass fifteen centimetres below the hip. [[unpin]]

## lever: Gravity pulls with a lever
[[scroll block:gravity-pulls-with-a-lever]] Gravity pulls the leg straight down. <short pause> How much that turns the hip depends on how far out sideways the weight is.
[[box figure:leg-angles "the lever arm"]] Hanging, no lever at all. Level, the whole distance. [[unmark]] [[quiz hardest]]
[[highlight "The lever arm is r·sin φ"]] So the torque is m g r, times the sine of the angle. [[highlight off]] [[quiz peak-torque]]

## sag: A servo sags
[[scroll block:a-servo-holds-by-leaving-an-error]] [[highlight "like a torsion spring"]] Now, a position servo pushes toward its target like a spring. [[highlight off]]
<short pause> So it can only push back against a load by being a little off target. The heavier the load, the more it sags. [[quiz sag]]

## lift: Lifting it
[[scroll quiz:predict-level]] Now let's lift the leg to level. Where will it settle? [[quiz predict-level]]
[[scene lift]] [[scroll scene:lift]] [[play-until 1.0]] Near hanging, gravity barely resists. [[play-until 1.9]] Higher up, its lever grows, and the servo pushes harder.
[[play]] [[wait-scene]] And at level, the leg settles four degrees below its target. [[quiz why-lag]]

## stall: When it can't hold
[[scroll block:when-it-cannot-hold]] [[highlight "stall torque"]] Every servo has a stall torque: the most it can make at all. [[highlight off]]
If gravity needs more, no error is big enough. The leg just stops, partway up. [[quiz max-mass]]

## task: Fix the leg
[[scroll block:fix-the-leg]] Now a real puzzle. The leg got heavier, and the servo can't lift it. <short pause> Can you fix the leg, without touching the servo?

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain why a servo that exactly matches the leg's weight still isn't enough.
[[scroll block:key-ideas]] [[highlight "Gravity torque"]] Gravity's torque is largest level. [[highlight "sagging"]] A servo holds by sagging. [[highlight off]] And always keep a margin.
