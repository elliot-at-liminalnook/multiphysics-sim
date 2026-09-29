---
voice: Kore
style: A warm, unhurried physics teacher talking one-to-one with a curious engineer. Leave room between ideas.
---

## hook: A joint that ignores you
[[scroll top]] Tell a small robot joint to move a little, and often, <short pause> nothing happens. The motor hums, and the joint stays put.
[[pin part:friction "gearbox friction"]] The culprit is friction. Let's see exactly how it works. [[unpin]]

## dry: Dry friction holds
[[scroll block:dry-friction-holds]] [[highlight "always opposes the motion"]] Dry friction is a torque of fixed size that always opposes the motion.
[[highlight "Push a still shaft with less than"]] And here's the trouble: push a still shaft with less than that, and friction pushes back exactly as hard. Nothing moves. [[highlight off]] [[quiz holds]]

## free: Where it breaks free
[[scroll block:where-the-motor-breaks-free]] So when does the motor break free? <short pause> A still motor has no back-EMF, so the whole commanded voltage drives current through the winding. [[quiz still-current]]
[[highlight "That current makes a torque"]] That current makes torque. When the torque reaches the friction, the shaft moves. [[highlight off]]
[[highlight "That flat stretch is the"]] For our joint, any command under about twenty-one percent does nothing at all. That's the dead band. [[highlight off]] [[quiz dmin]]

## sweep: The sweep
[[scroll quiz:sketch-sweep]] Now we'll sweep the command slowly up and down. Sketch what you expect first. [[quiz sketch-sweep]]
[[scene sweep]] [[scroll scene:sweep]] [[play-until 0.45]] The command rises, the current rises, <short pause> and the shaft doesn't move.
[[play-until 0.8]] Past about point two, it breaks free. [[play]] [[wait-scene]] And on the phase plot, that flat stretch around zero is the dead band.
[[quiz current-without-motion]]

## viscous: Viscous friction
[[scroll block:viscous-friction-is-different]] There's a second kind of friction. [[highlight "a drag proportional to speed"]] Oil, air, and the motor's iron make a drag proportional to speed. [[highlight off]]
[[box figure:deadband "two frictions"]] It tilts the line, but it never holds anything still. [[unmark]] [[quiz which-friction]]

## control: Position control
[[scroll block:what-it-does-to-position-control]] Here's why it matters for a robot. A position controller's command shrinks as the joint nears its target. <short pause> Once it falls inside the dead band, the joint just stops short. [[quiz stops-short]]
[[highlight "The usual fixes"]] The fixes: add the friction back as feed-forward, add integral action, or add a small dither. [[highlight off]]

## own-words: In your own words
[[scroll block:putting-it-in-your-own-words]] Last step. Explain why a servo can hum and warm up while standing still.
[[scroll block:key-ideas]] [[highlight "Dry friction"]] Dry friction holds. [[highlight "dead band"]] Below the threshold, nothing moves, <short pause> but current still flows. [[highlight off]]
