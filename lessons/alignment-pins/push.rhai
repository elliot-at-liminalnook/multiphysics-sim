// The plate is pushed ±20 N sideways once every 2 s. Around each force
// reversal (t = 1, 2, 3 s) it crosses the clearance in about 40 ms, then
// meets the pin: those spans play in deep slow motion.
zoom("", 1.6, 1.0);
speed(0.5);
caption("The hand pushes the plate sideways; the pin holds it");
highlight("pin");
at(0.96);
speed(0.02);
caption("The push reverses: the plate slides across the clearance with nothing holding it");
highlight("plate");
at(1.08);
speed(0.5);
caption("Against the pin's other side: now it is stiff, and barely moves further");
highlight("pin");
at(1.96);
speed(0.02);
caption("And back across the gap again");
highlight("plate");
at(2.08);
speed(0.6);
at(2.97);
speed(0.03);
at(3.08);
speed(0.6);
