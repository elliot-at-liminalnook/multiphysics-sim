---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: Why legs are slow
[[scroll top]] A robot leg's motor can have plenty of torque, <short pause> and the leg still swings slowly. Let's find out why, on the simplest machine there is: a flywheel.
[[scroll block:torque-makes-speed-change]] First, a guess. [[quiz guess-double]]

## rule: Torque changes speed
[[highlight "It sets how quickly the speed"]] Here's the key idea. A torque doesn't set a speed. <short pause> It sets how quickly the speed changes.
[[highlight off]] How much change you get depends on the inertia, J. <short pause> Torque equals inertia times angular acceleration.
[[highlight "Twice the inertia, half the acceleration"]] Twice the inertia, half the acceleration. And with no torque at all, the speed just stays where it is.
[[highlight off]] Try one. [[quiz alpha]]

## sketch: Draw it first
[[scroll quiz:sketch-spin]] Before we watch, draw what you think the speed will do. <short pause> There's no friction at all. [[quiz sketch-spin]]
Now commit to a number. [[quiz predict-speed]]

## watch: The push
[[scene spin-up]] [[scroll scene:spin-up]] [[pin part:wheel "the flywheel"]] [[play-until 1.05]] The push starts, and the speed climbs in a straight line. <short pause> The same torque, the same acceleration, every moment.
[[box plot:wheel.shaft.speed@0.1..1.1 "straight ramp"]] A steady torque gives a straight ramp.
[[unmark]] [[play]] [[wait-scene]] [[unpin]] And when the push stops, <short pause> nothing changes the speed any more. It just stays at a hundred radians per second.
[[quiz after-push]]

## where: Where the mass sits
[[scroll block:where-the-mass-sits]] [[box figure:inertia-shapes "same mass"]] Now, inertia isn't just mass. It's where the mass sits.
[[unmark]] Each bit of mass counts by the square of its distance from the axis. <short pause> So a ring, with all its mass at the rim, has twice the inertia of a disc. [[quiz ring-vs-disc]]

## ring: Disc and ring
[[scroll quiz:predict-ring]] Let's check that. Same push, on a disc and on a ring of the same mass. [[quiz predict-ring]]
[[scene ring]] [[scroll scene:ring]] [[play]] [[wait-scene]] Half the speed. Twice the inertia, half the acceleration.

## legs: Legs
[[scroll block:legs-mass-at-the-end-costs-most]] Now, a leg. It's like a rod swinging about the hip, with its foot at the far end. [[quiz leg-steps]]
<short pause> That little servo at the end added sixty percent. [[quiz move-servo]]
[[highlight "put their motors near the hips"]] That's why walking robots keep their motors near the hips, and drive the lower joints with belts or linkages.

## own-words: In your own words
[[highlight off]] [[scroll block:putting-it-in-your-own-words]] Last step: explain it in your own words. It's the step that makes it stick.
[[scroll block:key-ideas]] [[highlight "A torque sets how fast the speed"]] Torque changes speed. [[highlight "Inertia"]] And inertia counts every bit of mass by its distance, squared. [[highlight off]]
