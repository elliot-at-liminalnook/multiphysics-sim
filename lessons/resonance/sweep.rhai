// Presentation for the sweep: frequency = 2 + 4·t Hz. Slow enough
// throughout to see the swings; slowest through resonance near 2 s.
zoom("payload", 1.5, 1.0);
speed(0.2);
caption("Slow shaking (2 Hz): the payload just follows the push");
highlight("shaker");
at(1.2);
speed(0.05);
caption("Nearing 10 Hz: each push arrives in step with the swing");
highlight(["payload", "mount"]);
at(2.1);
caption("Resonance: about eight times the steady bend");
pause();
at(2.7);
speed(0.25);
caption("Faster than f_n: the payload can no longer keep up");
highlight("payload");
