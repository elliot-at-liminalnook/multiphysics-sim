---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: Robots hit things
[[scroll top]] Walking robots hit things all the time. <short pause> Every step is a small collision with the ground.
[[pin part:rotor "the rotor, through the gearbox"]] And behind every geared joint sits a rotor that, seen through the gearbox, is surprisingly heavy. [[unpin]]

## spring: Spring torque
[[scroll block:spring-torque]] First, the spring. [[highlight "it pushes back with"]] Twist a torsion spring, and it pushes back in proportion to the twist. [[highlight off]] [[quiz twist]]

## sensor: A torque sensor
[[scroll block:a-torque-sensor-made-of-a-spring]] [[box figure:sea "motor, spring, arm"]] Now put an encoder on each end. <short pause> The difference between the two angles is the twist, and the twist, times k, is the torque. [[unmark]]
That's a series-elastic actuator. [[quiz sensor-choice]]

## wall: Into the wall
[[scroll quiz:predict-impact]] Now we'll swing the arm into a wall: once through the soft spring, once through a stiff coupling. Which hits harder? [[quiz predict-impact]]
[[scene wall]] [[scroll scene:wall]] [[play-until 0.7]] The arm swings toward the wall. [[play-until 0.96]] Impact. <short pause> With the stiff coupling, a spike: the gear teeth take it all.
[[play]] [[wait-scene]] With the spring, the rotor keeps turning and winds it up, and the torque rises gently.

## why: Why so different
[[scroll block:why-the-stiff-one-hits-so-hard]] [[highlight "twelve times the arm itself"]] Seen through the gearbox, the rotor is twelve times heavier than the arm itself.
[[highlight "the rotor must stop with it"]] With a stiff coupling, when the arm stops, the rotor must stop with it, <short pause> almost instantly.
[[highlight "It keeps turning, winding the spring up"]] With a spring, it doesn't have to. It slows down over a tenth of a second instead of a millisecond. [[highlight off]] [[quiz why-soft]]

## price: The price
[[scroll block:the-price-of-softness]] But a soft spring is a sloppy connection. <short pause> The arm bounces on it, and fast precise moves get harder. [[quiz price]]

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain why walking robots put springs in their legs, and what it costs them.
[[scroll block:key-ideas]] [[highlight "carries"]] A spring's twist is a torque. [[highlight "slow gradually"]] And it lets the heavy rotor slow gradually. [[highlight off]]
