// Presentation for the sweep: the break-free moments at 0.5 s and 4.5 s
// are the story, so playback slows around them; the long runs in between
// play at full speed.
zoom("load", 1.5, 1.0);
speed(0.3);
caption("The command rises from zero; the motor draws current, but friction holds");
highlight("friction");
at(0.45);
caption("Past about 0.21 the motor's torque beats friction: it breaks free");
highlight("load");
at(0.8);
speed(1.0);
caption("Beyond the dead band, the speed follows the command");
zoom("motor", 1.3, 1.5);
at(3.6);
speed(0.3);
caption("Back through zero: stuck again for commands under 0.21");
highlight("friction");
at(4.7);
speed(1.0);
caption("The same on the negative side");
