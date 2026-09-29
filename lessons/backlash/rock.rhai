// Presentation for the rocking scene: each reversal (0.25, 0.75, 1.25,
// 1.75 s) is where the pointer waits; the first two play slowly.
zoom("", 1.9, 1.0);
speed(0.3);
caption("The motor side rocks the pointer through the gears");
highlight("servo");
at(0.22);
speed(0.12);
caption("The motor reverses: the pointer waits while the teeth cross the gap");
highlight("mesh");
at(0.42);
speed(0.3);
caption("Teeth touching again: the pointer follows, 0.04 rad behind");
highlight("pointer");
at(0.72);
speed(0.12);
caption("And again at the other end");
highlight("mesh");
at(0.92);
speed(0.5);
