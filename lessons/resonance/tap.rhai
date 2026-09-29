// Presentation for the tap: the 10 ms push, then 10 Hz ringing slowed so
// each swing can be seen.
zoom("payload", 1.6, 1.0);
speed(0.05);
caption("A sharp 10 ms push on the payload");
highlight("drive");
at(0.13);
caption("Then nothing: it rings at its natural frequency, about 10 Hz");
highlight(["payload", "mount"]);
at(0.45);
speed(0.1);
caption("Each swing about 27 % smaller: the damper takes energy out");
highlight("damper");
at(0.9);
speed(0.25);
