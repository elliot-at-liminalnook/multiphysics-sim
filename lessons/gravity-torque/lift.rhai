// Presentation for the lift: the leg rises for 1.6 s; captions point at
// gravity's growing lever and the final sag.
zoom("leg", 2.0, 1.0);
speed(0.5);
caption("Hanging: gravity pulls straight through the hip");
highlight("leg");
at(0.3);
caption("The target starts rising; the servo pushes a little");
highlight("servo");
at(1.0);
caption("Higher up, gravity's lever grows: the servo pushes harder");
highlight("leg");
at(1.87);
caption("Level: the target stops, the leg settles 4° below it");
highlight("servo");
pause();
