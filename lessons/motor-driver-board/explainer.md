---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## board: Meet the board
[[scroll top]] This little board has two jobs. <short pause> [[arrow part:regulator "regulator"]] One part makes a steady five volts for the logic chips. [[arrow part:bridge "H-bridge"]] The other drives a motor, in either direction.
[[unmark]] We'll look inside each one, <short pause> one idea at a time. [[quiz board-jobs]]

## resistor: Why not a resistor
[[scroll block:why-not-a-resistor]] First question: why not just use a resistor to drop eleven volts down to five?
[[highlight "the resistor carries the full current"]] The problem is that the resistor carries all the current, <short pause> so every volt it drops turns into heat.
[[highlight off]] In the worked example, more than half the battery's power is thrown away. Try the next one. [[quiz resistor-waste]]

## switch: A switch
[[scroll block:a-switch-wastes-almost-nothing]] Now picture a switch instead. [[highlight "fully on, there is almost no voltage across it"]] Fully on, there's almost no voltage across it. [[highlight "fully off, almost no current goes through it"]] Fully off, almost no current through it.
[[highlight off]] Either way, <short pause> it barely warms up. [[quiz switch-loss]]

## average: Chopping and averaging
[[scroll block:chopping-and-averaging]] But a switch gives either all eleven volts, or nothing. <short pause> So we switch it fast, and take the average.
[[highlight "the average voltage is D times the input"]] On for half the time, you get half the voltage, on average. [[highlight off]] What fraction do we need for five volts? [[quiz duty-cycle]]

## smooth: Smoothing
[[scroll block:smoothing-out-the-chopping]] [[box figure:buck@60,260,660,110 "switch node"]] After the switch, the voltage is a fast square wave. <short pause> [[arrow figure:buck@80,316,620,16 "average"]] The inductor and capacitor keep only its average.
[[unmark]] Before we switch the board on, one prediction. [[quiz sketch-rail]]
[[scene power-up]] [[scroll scene:power-up]] [[play]] Here it is, slowed right down. [[box plot:regulator/c.p.voltage "5 V rail"]] The rail climbs, and settles.
[[wait-scene]] [[unmark]] Flat, within a tenth of a volt, even with the motor running.

## heat: Heat in the driver
[[scroll block:heat-in-the-motor-driver]] [[arrow part:bridge/heatsink "heatsink"]] The motor driver uses the same kind of switches. <short pause> They're not quite perfect: each has a tiny resistance when on.
[[unmark]] [[highlight "it grows fast with current"]] So they make a little heat, and it grows fast with current. [[highlight off]] [[quiz bridge-heat]]
[[scroll quiz:why-switch]] [[box quiz:why-switch "your turn"]] Finally, explain it back in your own words, just below. <short pause> That's where it really sinks in.
