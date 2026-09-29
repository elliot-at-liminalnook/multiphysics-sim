// Presentation for brake against coast: spin-up to 0.4 s, then the stop.
// The braking current appears within a millisecond of 0.4 s.
zoom("", 1.6, 1.0);
speed(0.25);
caption("Full forward drive: the wheel spins up");
highlight("bridge");
at(0.3);
speed(0.03);
at(0.395);
speed(0.001);
at(0.4);
caption("Stop driving. Left: both low switches on (brake). Right: all off (coast)");
highlight("bridge");
pause();
at(0.402);
speed(0.03);
at(0.45);
speed(0.25);
caption("The braked wheel slows fast; the coasting one hardly at all");
highlight("load");
