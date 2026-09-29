// Presentation for the generator brake: the current appears within a
// fraction of a millisecond (L/R), then speed and current decay together.
zoom("", 1.6, 1.0);
speed(0.002);
caption("The wheel spins the motor at 600 rad/s: 7.2 V of back-EMF");
highlight("motor");
at(0.002);
speed(0.1);
caption("Through the resistor, 2.4 A flows: its torque brakes the wheel");
highlight(["brake", "load"]);
at(0.6);
speed(0.25);
caption("Slower wheel, less voltage, less current, gentler brake");
highlight("load");
