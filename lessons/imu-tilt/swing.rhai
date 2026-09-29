// Presentation for the IMU: the kick at 0.2–0.34 s (its edges change the
// accelerometer's reading fast), the swing (about 1 Hz) in slow motion,
// then the settling while the gyro drifts.
zoom("leg", 1.3, 1.0);
speed(0.25);
caption("The leg hangs still; the IMU rides 18 cm below the hip");
highlight("imu");
at(0.15);
speed(0.02);
at(0.2);
caption("A kick: the leg swings, and the IMU accelerates with it");
highlight("leg");
at(0.23);
speed(0.08);
at(0.3);
speed(0.02);
at(0.37);
speed(0.1);
caption("Swinging: the accelerometer barely sees the tilt; the gyro follows it");
highlight("imu");
at(2.0);
speed(0.5);
caption("Settling: the gyro's angle keeps drifting away, 0.05 rad every second");
highlight("filter");
