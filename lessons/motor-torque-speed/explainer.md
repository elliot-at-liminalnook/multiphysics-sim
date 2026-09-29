---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## meet: Meet the motor
[[scroll top]] Let's build up how a motor works, <short pause> one small idea at a time.
[[scroll block:meet-the-motor]] [[box figure:motor-model@20,40,360,250 "electrical side"]] A motor has two sides. On the left, the electrical side: a battery, pushing current through a coil of wire.
[[unmark]] [[box figure:motor-model@420,40,280,250 "mechanical side"]] On the right, the mechanical side: a shaft, turning a load.
[[unmark]] That's all for now. <short pause> Just two sides. [[quiz two-sides]]

## torque: Current makes torque
[[scroll block:current-makes-torque]] First idea. [[highlight "the torque is simply proportional to the current"]] When current flows through the coil, the magnets push on it, and the shaft turns. More current, more push.
[[highlight off]] The number that links them is the torque constant, k. <short pause> For our motor, point zero one two newton metres for every amp.
[[highlight "With 2 A in the winding"]] So two amps gives point zero two four. <short pause> Your turn. [[quiz torque-from-current]]

## emf: Spinning makes a voltage
[[highlight off]] [[scroll block:spinning-makes-a-voltage]] Second idea. <short pause> Run it backwards.
[[highlight "If you spin the shaft by hand"]] If you spin the shaft yourself, the motor becomes a generator, and makes a voltage. We call it the back-EMF.
[[highlight "with the same constant k"]] And here's the lovely part: <short pause> it uses the SAME constant, k. Same magnets, same coil.
[[highlight off]] Try one. [[quiz back-emf-at-speed]]

## budget: The voltage budget
[[scroll block:the-voltage-budget]] Now we put the two ideas together. [[highlight "think of it as a budget"]] Think of the battery's twelve volts as a budget.
[[highlight off]] Part of it is taken by the back-EMF. <short pause> Whatever is left pushes current through the winding.
[[highlight "the more of the budget the back-EMF takes"]] So the faster the motor spins, the less is left over for current, <short pause> and the less torque you get.
[[highlight off]] Work through this one slowly. [[quiz current-at-speed]]

## stall: Held still
[[scroll block:held-still-stall]] Now the two extremes. <short pause> First: hold the shaft so it can't turn. [[quiz stall-current]]
<short pause> Exactly. With no spinning, there's no back-EMF, <short pause> so the current is the biggest it will ever be.

## free: Spinning free
[[scroll block:spinning-free-no-load]] The other extreme: nothing attached at all.
Almost no current flows, so the back-EMF takes the whole budget. <short pause> How fast is that? [[quiz no-load-speed]]

## line: A straight line
[[scroll block:a-straight-line-between-the-ends]] [[box figure:torque-speed "torque–speed line"]] So now you know both ends. <short pause> And in between, it's a straight line.
[[unmark]] [[arrow figure:torque-speed@60,52,40,40 "stall"]] Up here, stall: the most torque.
[[arrow figure:torque-speed@640,340,40,40 "no load"]] Down here, spinning free.
[[unmark]] [[arrow figure:torque-speed@478,260,40,40 "A"]] And point A is our motor with a light load. [[quiz line-midpoint]]

## predict: Make a prediction
[[unmark]] [[scroll quiz:predict-doubled]] In the scene, the motor starts at point A. <short pause> Then the load doubles.
Before you watch, commit to a number. [[quiz predict-doubled]]
Good. <short pause> Hold on to that guess.

## spin-up: Watching it spin up
[[unmark]] [[scene load-step]] [[scroll scene:load-step]] [[zoom motor 1.6 over=1.5]] [[pin part:rotor "the rotor"]] Keep an eye on the rotor. [[play-until 0.3]] When it switches on, current rushes in, and the rotor races up to speed.
[[box plot:rotor.shaft.speed@0..0.3 "spin-up"]] See how the speed rises, and then levels off?
[[wait-scene]] [[unpin]] [[zoom all 1]] It settles where the motor's torque matches the load.

## load: Doubling the load
[[unmark]] [[arrow part:load "the load"]] Now, the load doubles. [[play]] [[box plot:motor.p.current@0.3..0.6 "current"]] The current jumps up, <short pause> and the rotor slows.
[[wait-scene]] [[box plot:rotor.shaft.speed@0.3..0.6 "new speed"]] Then it settles again. <short pause> How close was your guess?
[[unmark]] [[quiz current-follows-load]] <short pause> The motor doesn't try harder. It just has less back-EMF holding the current back.

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] [[orbit 0.15]] Last step, and it's the one that makes it stick. [[scroll quiz:why-stall-hot]] Explain, in your own words, why a jammed motor can burn out.
[[scroll block:key-ideas]] [[highlight "Current makes torque"]] Current makes torque. [[highlight "Spinning makes a voltage"]] Spinning makes a voltage. <short pause> [[highlight "The voltage budget"]] And the budget between them draws one straight line. [[highlight off]] [[orbit off]]
