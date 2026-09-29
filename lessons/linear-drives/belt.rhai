// Presentation for the belt lift: the slide down lasts about 0.2 s.
zoom("", 1.15, 0.8);
speed(0.002); // the winding current rises within a millisecond
caption("Full voltage: the motor pulls up with about 11 N");
at(0.0015);
speed(0.05);
highlight("motor");
at(0.03);
caption("The 19.6 N weight wins: the carriage slides down, spinning the motor backwards");
highlight(["carriage", "weight"]);
at(0.2);
speed(0.2);
caption("It comes to rest on its lower stop");
highlight("stops");
