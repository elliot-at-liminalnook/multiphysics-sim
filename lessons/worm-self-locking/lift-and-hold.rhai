// Presentation for the lift-and-hold scene. Times are simulated seconds.
// Only set() would change physics; this script only guides the eye.
// Playback slows around the spin-up and the power-off so each can be
// followed (the viewer shows the speed); captions hold for reading.
speed(0.04);
caption("12 V on: the motor spins up and the drum starts lifting");
highlight("motor");
wait(0.15);
speed(0.5);
wait(0.25);
caption("Steady lift: motor torque balances the load through the worm");
highlight(["gearbox/worm", "drum"]);
at(1.1);
speed(0.1);    // ease into slow motion before the power-off
at(1.19);
speed(0.004);  // the current collapses within a millisecond of power-off
at(1.2);
caption("Power off at 1.2 s");
camera("side", "gearbox");
highlight("gearbox/mesh");
pause();
speed(0.004);
wait(0.012);
speed(0.04);
wait(0.138);
speed(0.5);
wait(0.05);
caption("The drum stays put: nothing is braking it except the thread's friction");
