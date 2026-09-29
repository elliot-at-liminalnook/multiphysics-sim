// The plate is placed on two pins whose spacing is 0.2 mm too large. It
// settles within about 3 ms; what is left is the force the pins lock in.
zoom("", 1.4, 1.0);
speed(0.004);
caption("The plate is pushed onto both pins: the second pin is 0.2 mm off");
highlight("pin2");
at(0.0035);
speed(0.0015);
at(0.0075);
speed(0.05);
caption("Settled: each pin now pushes on the plate, with no load applied at all");
highlight("pin1");
at(0.02);
speed(0.4);
