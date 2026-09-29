---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: It worked in simulation
[[scroll top]] A controller is tuned in a fast simulation, and works beautifully. <short pause> On the real robot, the same gains make the leg shake. Why?

## sampled: Sampled and held
[[scroll block:sampled-and-held]] [[highlight "Between steps the controller is blind"]] Firmware runs in steps. Between steps, it's blind: the arm moves on, the command doesn't. [[highlight off]]
On average, the command is half a period old. [[quiz half-period]]

## latency: Latency
[[scroll block:latency-adds-on-top]] [[box figure:delay "old news"]] And reading, computing and sending all take time too. That latency adds straight on. [[unmark]] [[quiz total-delay]]

## why: Why delay shakes a joint
[[scroll block:why-delay-shakes-a-joint]] [[highlight "it sees where the arm"]] With a delay, the controller sees where the arm was. <short pause> If the arm has swung past, it keeps pushing the old way, and feeds the swing. [[highlight off]] [[quiz predict-slow]]
[[scene slow-loop]] [[scroll scene:slow-loop]] [[play-until 0.6]] The target steps, and the controller pushes, on old news. [[play]] [[wait-scene]] And it never settles. Same gains that were perfect at a kilohertz. [[quiz fix]]

## sim: Simulate the real loop
[[scroll block:simulate-the-loop-you-really-have]] So simulate the loop you really have: its real rate, its real latency. <short pause> Then you'll see the price before the robot does.

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain why a controller can work in simulation and shake on the robot.
[[scroll block:key-ideas]] [[highlight "T/2 + latency"]] A sampled loop acts on old news. [[highlight "Simulate the real loop rate"]] So simulate the real loop, and tune there. [[highlight off]]
