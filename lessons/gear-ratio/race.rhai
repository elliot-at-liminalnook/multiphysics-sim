// Presentation for the race scene: both motors switch on at 0 s, and the
// interesting part is the first 60 ms. Slow motion throughout the start.
zoom("", 1.7, 0.8);
speed(0.02);
caption("Switch on: each motor pulls 6 A at stall");
highlight("motor");
at(0.012);
caption("Through the gear, the load swings away; direct, it barely moves");
highlight("load");
at(0.05);
speed(0.05);
caption("The geared load nears its 200 rad/s top speed");
at(0.08);
speed(0.1);
