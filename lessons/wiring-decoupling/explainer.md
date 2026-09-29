---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: The board that squeals
[[scroll top]] A motor driver works on a short lead, <short pause> but on a long one it squeals, resets, or even blows a capacitor. Why would a metre of wire matter? [[quiz guess-culprit]]

## r: A wire is a resistor
[[scroll block:a-wire-is-a-resistor]] First, every wire has some resistance, and it costs voltage in proportion to the current. [[quiz dc-drop]]

## l: A wire is an inductor
[[scroll block:a-wire-is-also-an-inductor]] [[highlight "fights any change in its current"]] But a wire is also an inductor. It fights any change in its current. [[highlight off]]
<short pause> It doesn't care how big the current is, only how fast it changes. [[quiz spike]]

## chop: The chopped current
[[scroll block:the-chopped-current]] [[box figure:leads "supply, lead, board"]] And a switching driver changes its current constantly: five amps on, five amps off, twenty thousand times a second. [[unmark]] [[quiz predict-bounce]]
[[scene switching]] [[scroll scene:switching]] [[play-until 0.0002]] Switch on: the lead can't deliver the current fast enough, and the board dips. [[play]] [[wait-scene]] Switch off: the lead's current has nowhere to go, and it drives the board above twelve volts.
[[quiz why-spike]]

## cap: A capacitor at the board
[[scroll block:a-capacitor-at-the-board]] [[highlight "a local reservoir"]] The fix is a capacitor right at the board's input: a local reservoir. [[highlight off]]
It supplies each pulse, <short pause> and the lead only has to carry the slowly changing average. That's decoupling. [[quiz size-cap]]

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain why a long lead upsets a switching driver, and what the capacitor does.
[[scroll block:key-ideas]] [[highlight "Decoupling"]] Keep the fast pulses local, and let the wires carry the average. [[highlight off]]
