// Presentation for five minutes of heating, played at about 20× so the
// whole climb fits in about 15 s.
zoom("", 1.3, 1.0);
speed(20);
caption("A steady 25 mN·m: 2.1 A through the winding, 8.7 W of heat");
highlight("motor");
at(60);
caption("One time constant (60 s): about two thirds of the way");
highlight("winding");
at(150);
caption("Hot copper resists more: the current and the heat creep up");
highlight("motor");
at(250);
caption("Still climbing past 115 °C: too hot for continuous running");
highlight("cooling");
