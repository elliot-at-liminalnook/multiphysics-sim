// Presentation for the slow loop: the step at 0.2 s, then the
// oscillation that never dies.
zoom("arm", 2.2, 1.0);
speed(0.3);
caption("Firmware at 50 Hz, 10 ms late; the target is about to step");
highlight("firmware");
at(0.15);
speed(0.04);
at(0.2);
caption("Target 1 rad: the controller pushes, on old news");
highlight("arm");
at(0.4);
caption("Past the target, it is still pushing the old way");
at(0.6);
speed(0.04);
caption("Each late push feeds the swing: it never settles");
highlight("firmware");
