// Presentation for 0.25 ms of 20 kHz switching (5 periods). The lead
// and the small capacitor ring at about 160 kHz, so playback is very
// slow: each 50 µs switching period takes about 10 s on screen.
zoom("", 1.5, 1.0);
speed(0.000005);
caption("The switch starts chopping: 20 000 times a second");
highlight("switch");
at(0.0001);
caption("Switch on: the lead cannot deliver fast enough, the board dips");
highlight(["wire_l", "cap"]);
at(0.0002);
caption("Switch off: the lead's current charges the small capacitor above 12 V");
highlight("cap");
at(0.00022);
caption("The motor's current stays steady: only the supply side is chopped");
highlight("motor");
