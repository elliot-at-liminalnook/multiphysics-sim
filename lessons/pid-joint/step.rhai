// Presentation for the PID step: the swing takes about 0.3 s after the
// step at 0.2 s; the command jumps at the step and swings as the D term
// brakes, so those spans play slowest.
zoom("arm", 2.2, 1.0);
speed(0.3);
caption("Holding at 0 rad, the target about to step to 1 rad");
highlight("target");
at(0.15);
speed(0.03);
at(0.198);
speed(0.008);
at(0.2);
caption("Target 1 rad: a large error, full drive");
highlight("arm");
at(0.216);
speed(0.04);
at(0.26);
speed(0.015);
caption("Arriving at the target: does it stop in time?");
at(0.3);
speed(0.06);
at(0.7);
speed(0.3);
caption("Settling: where does it end up, and why?");
highlight("pid");
