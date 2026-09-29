---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: One particular speed
[[scroll top]] A camera on a walking robot can shake badly at one walking pace, <short pause> and hardly at all at others. That's resonance. Let's see where it comes from.

## fn: The natural frequency
[[scroll block:the-natural-frequency]] Twist the payload and let go. <short pause> The spring pulls it back, it overshoots, and it swings by itself.
[[highlight "A stiffer spring pulls harder"]] A stiffer spring swings it faster; a heavier payload, slower. [[highlight off]] Our mount swings about ten times a second. [[quiz fn]]

## tap: A tap
[[scroll quiz:predict-tap]] Now we'll give it one sharp tap. What do you expect? [[quiz predict-tap]]
[[scene tap]] [[scroll scene:tap]] [[play]] [[wait-scene]] Ten swings a second, each one a little smaller.

## zeta: Damping
[[scroll block:how-fast-it-dies-away]] [[highlight "damping ratio"]] How fast the ringing dies is captured by one number: the damping ratio, zeta. [[highlight off]]
Ours is point oh five: light, like most bolted metal. [[quiz zeta]]

## sweep: Shaken at every frequency
[[scroll block:shaken-at-every-frequency]] Now we'll shake it gently, <short pause> with a frequency that climbs steadily, from two hertz upward. [[quiz predict-sweep]]
[[scene sweep]] [[scroll scene:sweep]] [[play-until 1.2]] At first, the payload just follows the push. [[play-until 2.2]] Then, near ten hertz, each push arrives just in time to add to the swing. <short pause> It grows about eight times larger.
[[play]] [[wait-scene]] And past the natural frequency, it can't keep up at all.

## peak: How tall
[[scroll block:how-tall-the-peak-gets]] [[box figure:response "amplification"]] How tall the peak gets depends on the damping: about one over two zeta. [[unmark]] [[quiz amplification]]

## design: Keeping out of it
[[scroll block:keeping-a-robot-out-of-resonance]] So a designer has three levers. <short pause> Move the natural frequency away, add damping, or stop shaking at that frequency. [[quiz gait]]

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain why an arm might shake at one speed and not at others.
[[scroll block:key-ideas]] [[highlight "natural frequency"]] Every spring and mass has a natural frequency. [[highlight "resonance"]] Push it there, and the motion builds up. [[highlight off]]
