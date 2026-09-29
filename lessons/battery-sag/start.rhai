// Presentation for the motor start: the current step at 0.1 s rises in
// about 0.1 ms (L/R); the pack recovers over the ~0.3 s spin-up.
zoom("", 1.3, 1.0);
speed(0.2);
caption("At rest: the pack sits at 8.0 V, the controller draws 0.2 A");
highlight("pack");
at(0.098);
speed(0.001);
at(0.1);
caption("Full drive into a still motor: 17 A, and the pack drops 2 V");
highlight(["motor", "pack"]);
pause();
at(0.1012);
speed(0.01);
at(0.12);
speed(0.05);
caption("The motor speeds up, its back-EMF grows, the current falls");
highlight("load");
at(0.3);
speed(0.2);
caption("The pack recovers toward 7.7 V");
highlight("pack");
