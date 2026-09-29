---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: The robot that resets
[[scroll top]] A small walking robot works perfectly on the bench supply. <short pause> On its battery, it resets every time it tries to stand up. Let's find out why. [[quiz guess-dip]]

## rint: Not a perfect source
[[scroll block:a-battery-is-not-a-perfect-source]] [[box figure:pack-model "inside the pack"]] Inside a battery, the chemistry makes a steady voltage, <short pause> but the current has to get out through a small resistance. [[unmark]]
[[highlight "Whatever current you draw, it takes its share"]] Whatever current you draw, that resistance takes its share of the voltage. [[highlight off]] [[quiz drop]]

## start: The worst moment
[[scroll block:the-start-is-the-worst-moment]] And the worst moment is a motor start. <short pause> A still motor has no back-EMF, so only the resistances limit its current. [[quiz start-steps]]
[[quiz predict-dip]]
[[scene start]] [[scroll scene:start]] [[play-until 0.102]] Full drive into a still motor: seventeen amps, <short pause> and the pack drops two volts. [[play]] [[wait-scene]] As the motor speeds up, the current falls, and the pack recovers.

## brownout: Brownout
[[scroll block:brownout]] [[highlight "a typical linear one needs about 6 V in"]] The controller's regulator needs about six volts in. [[highlight off]] Dip below that even for a millisecond, and the microcontroller resets. <short pause> That's a brownout. [[quiz reset]]

## fix: Keeping it alive
[[scroll block:keeping-the-controller-alive]] The dip is current times resistance, so there are two ways in: less resistance, or less current. <short pause> And you can give the controller its own margin. [[quiz four-motors]]

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain why a robot resets when it stands up, and what would fix it.
[[scroll block:key-ideas]] [[highlight "internal resistance"]] A battery is a source behind a resistance. [[highlight "brownout"]] And motor starts are when it sags the most. [[highlight off]]
