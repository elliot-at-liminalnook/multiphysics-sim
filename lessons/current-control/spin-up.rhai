// Presentation for the current loop: the current rises within about
// 2 ms of 10 ms; the spin-up is a straight line until about 0.63 s.
zoom("", 1.6, 1.0);
speed(0.002);
caption("Target 2 A at 10 ms: the loop drives the current up in about 2 ms");
highlight("loop");
at(0.009);
speed(0.0004);
at(0.0112);
speed(0.002);
at(0.014);
speed(0.02);
at(0.03);
speed(0.25);
caption("The wheel speeds up; the loop raises the voltage to hold 2 A");
highlight("load");
at(0.58);
speed(0.08);
at(0.63);
caption("Command at 100 %: the supply can no longer beat the back-EMF");
highlight("driver");
pause();
at(0.7);
speed(0.25);
