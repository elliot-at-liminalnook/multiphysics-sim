---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: Which way is down
[[scroll top]] A balancing robot has to know which way is down, <short pause> all the time, while it moves. It carries an IMU: an accelerometer and a gyro.

## acc: Gravity, measured sideways
[[scroll block:gravity-measured-sideways]] [[box figure:imu "gravity, split"]] Held still, an accelerometer feels gravity. Tilt it, and gravity's reading splits between its two axes. [[unmark]]
The tilt comes from their ratio. [[quiz acc-tilt]]

## fooled: Swinging fools it
[[scroll block:swinging-fools-the-accelerometer]] [[highlight "An accelerometer cannot tell gravity from acceleration"]] But an accelerometer can't tell gravity from acceleration. [[highlight off]] <short pause> On a swinging leg, the IMU accelerates too, and the tilt it reports is wrong. [[quiz why-wrong]]

## gyro: The gyro
[[scroll block:the-gyro-good-at-motion-bad-at-memory]] A gyro measures turning rate. Add it up over time, and it follows every swing. [[highlight "bias"]] But every gyro has a small bias, and added up, it grows without end. [[highlight off]] [[quiz drift]]

## watch: Watching both
[[scroll quiz:predict-both]] Let's watch both, on a swinging leg. [[quiz predict-both]]
[[scene swing]] [[scroll scene:swing]] [[play-until 0.37]] The kick: the accelerometer reads a tilt the leg doesn't have. [[play-until 1.0]] Swinging, it barely sees the real tilt, while the gyro follows it. [[play]] [[wait-scene]] And at rest, the gyro has drifted away. [[quiz during-swing]]

## blend: The filter
[[scroll block:the-complementary-filter]] They fail in opposite ways. <short pause> So a complementary filter takes each where it's good. [[quiz which-when]]
[[highlight "gently pulls the result toward the accelerometer"]] It follows the gyro, and gently pulls toward the accelerometer. [[highlight off]] [[quiz tau-choice]]
[[scroll scene:blend]] Now tune it, with a worse gyro.

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain why neither sensor alone can keep a robot upright.
[[scroll block:key-ideas]] [[highlight "complementary filter"]] Gyro for the short term, accelerometer for the long term. [[highlight off]]
