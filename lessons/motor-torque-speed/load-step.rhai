// The load doubles at 0.3 s (a physics input, recorded with the run).
// Playback slows around each fast change so it can be followed (the viewer
// shows the speed); captions also hold for reading (sim_script::pacing).
// The camera moves in on the motor while it spins up, then back out so the
// load is in view when it changes.
zoom("motor", 1.8, 1.0);
speed(0.0004); // the winding's current rises within about 0.25 ms (L/R)
caption("Switch on: current jumps, the rotor accelerates");
highlight("motor");
wait(0.001);
speed(0.004);
wait(0.002);
speed(0.02);   // the ~40 ms spin-up plays over about 2 s
wait(0.057);
speed(0.25);
wait(0.06);
caption("Back-EMF has grown; current falls to what the load needs");
highlight("rotor");
// Back out while the caption holds for reading, so the load is in view for its step.
zoom("", 1.3, 1.5);
at(0.3);
set("load.torque", -0.04);
caption("Load doubled: the rotor slows until torque balances again");
highlight("load");
pause();
speed(0.02);   // the slow-down after the load step
wait(0.06);
speed(0.25);
