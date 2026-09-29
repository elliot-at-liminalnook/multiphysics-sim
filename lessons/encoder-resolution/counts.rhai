// Presentation for the encoder: slow start (counts rare, estimate spiky),
// then faster (counts crowd together, estimate steadier).
zoom("", 1.8, 1.0);
speed(0.2);
caption("A gentle push: the wheel speeds up slowly");
highlight("wheel");
at(0.3);
speed(0.05);
caption("Slow: a count every 10 ms or more, each one kicks the estimate");
highlight("estimate");
at(1.0);
speed(0.15);
caption("Faster: counts crowd together within the 5 ms filter, and it steadies");
highlight("encoder");
