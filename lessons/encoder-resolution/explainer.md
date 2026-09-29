---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: A jumpy speed
[[scroll top]] A robot joint turns slowly and smoothly, <short pause> yet its speed reading jumps between zero and thirty. Let's find out why.

## counts: Whole counts
[[scroll block:an-angle-in-whole-counts]] [[highlight "It counts marks going past"]] An encoder counts marks going past. Between two counts, it can't see any motion at all. [[highlight off]]
Ours has sixty-four counts a turn: one count is five and a half degrees. [[quiz resolution]]

## speed: Speed from counts
[[scroll block:speed-from-counts]] [[box figure:staircase "staircase and spikes"]] Firmware works out speed from how much the angle changed recently. <short pause> When counts come slowly, each one arrives as a jump, and the speed estimate spikes. [[unmark]] [[quiz spike-size]]

## watch: Watching it
[[scroll quiz:predict-noise]] What will the estimate look like at ten radians per second? [[quiz predict-noise]]
[[scene counts]] [[scroll scene:counts]] [[play-until 1.0]] Slow: a row of spikes around the true speed. [[play]] [[wait-scene]] Faster, the counts crowd together, and it steadies. [[quiz filter-cost]]

## choose: Choosing resolution
[[scroll block:choosing-resolution]] [[highlight "more counts per turn shrink them directly"]] More counts shrink the spikes, without adding any delay. [[highlight off]] That's why robot joints use high-resolution encoders.

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain why a joint's speed reading can jump at low speed.
[[scroll block:key-ideas]] [[highlight "resolution"]] Counts set the resolution. [[highlight "noise against delay"]] And speed from counts trades noise against delay. [[highlight off]]
