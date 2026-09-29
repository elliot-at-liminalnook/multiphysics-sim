// The same growing pull (150 N per second) on both screws. The self-tapped
// one tears out at about 2.63 s, the insert at about 5.73 s; each tear takes
// a few milliseconds, so those spans play in very slow motion.
zoom("", 1.5, 1.0);
speed(0.6);
caption("The same pull, growing steadily, on a self-tapped screw and on a heat-set insert");
highlight("tapped");
at(2.45);
speed(0.05);
caption("The self-tapped thread is about to tear out of the plastic");
highlight("tapped_screw");
at(2.6);
speed(0.004);
at(2.67);
speed(0.05);
at(2.8);
speed(0.6);
caption("The insert holds on, gripping a wider cylinder of plastic");
highlight("insert");
at(5.55);
speed(0.05);
caption("Now the insert tears out too, at more than twice the force");
highlight("insert_screw");
at(5.70);
speed(0.004);
at(5.77);
speed(0.05);
at(5.9);
speed(0.6);
