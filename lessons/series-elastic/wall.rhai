// Presentation for the wall scene: the swing, then the impact at about
// 0.74–0.8 s in slow motion, then the spring winding up against the wall.
speed(0.5);
caption("The servo swings the arm toward the wall at 0.8 rad");
highlight("arm");
at(0.68);
speed(0.05);
caption("Impact: the arm stops at the wall");
highlight("wall");
at(0.8);
caption("The rotor keeps turning, winding the spring up");
highlight(["rotor", "spring"]);
at(0.96);
speed(0.25);
at(1.2);
caption("At rest: the spring's twist holds 1.09 N·m against the wall");
highlight("spring");
