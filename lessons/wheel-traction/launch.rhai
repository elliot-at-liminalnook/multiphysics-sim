// Presentation for the launch: spin (0.05–0.2 s), grip, brake at 0.4 s,
// skid (0.4–0.55 s). The rover travels about 0.47 m.
speed(0.05);
caption("Full throttle: far more torque than the tyre's grip can use");
highlight("rover/hub");
at(0.08);
caption("The wheel spins: its rim runs ahead of the rover");
at(0.2);
speed(0.15);
caption("The rover catches up with its wheel: now it grips");
highlight("rover/body");
at(0.38);
speed(0.03);
at(0.4);
caption("Brake: the motor is shorted, the wheel slows faster than the rover");
highlight("rover/hub");
pause();
at(0.43);
caption("A skid: the rim is slower than the ground passing under it");
at(0.6);
speed(0.2);
