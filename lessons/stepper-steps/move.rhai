// Presentation for the stepper move: the slip happens within 20 ms of
// the start at 0.05 s, so that span plays very slowly.
zoom("", 1.6, 1.0);
speed(0.05);
caption("The driver starts moving the target");
highlight("move");
at(0.05);
speed(0.004);
caption("The rotor tries to follow: it needs J·α of torque");
highlight("load");
at(0.066);
caption("Two steps behind: the pull reverses, and it slips");
highlight("stepper");
pause();
at(0.08);
speed(0.03);
caption("It only buzzes now, while the target races on");
highlight("load");
at(0.16);
speed(0.01);
at(0.18);
speed(0.1);
