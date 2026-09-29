// Presentation for the spin-up scene. Only set() would change physics;
// this script only guides the eye. The push switches on and off over
// about 20 ms: those edges play slowly; the one-second ramp at half speed.
zoom("wheel", 1.5, 1.0);
speed(0.05);
caption("The torque source pushes with a steady 13 mN·m");
highlight("drive");
at(0.13);
speed(0.5);
caption("The flywheel speeds up at a constant rate: α = τ/J");
highlight("wheel");
at(0.98);
speed(0.05);
at(1.1);
caption("The push ends. No torque, no friction: the speed stays at 100 rad/s");
highlight("wheel");
pause();
at(1.14);
speed(0.5);
