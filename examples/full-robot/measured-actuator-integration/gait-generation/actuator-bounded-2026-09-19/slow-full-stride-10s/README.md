# Experimental bounded full-stride candidate

Simulation only; **not promoted**. All four feet lift repeatedly, but the worst motor exceeds the frozen 2-degree RMS gate.

Ten seconds: 0.327 m net travel, 0.0327 m/s. Minimum body-up projection 0.997146. No fall. All reference derivatives remain inside 80 degrees/s and 400 degrees/s².

| Motor output | RMS error (degrees) | Peak error (degrees) | Pass tracking |
|---|---:|---:|---|
| -Y / Hip servo output | 0.489 | 1.122 | yes |
| -Y / Worm servo output | 0.277 | 0.839 | yes |
| -Y / Foot servo output | 2.423 | 3.851 | no |
| +X / Hip servo output | 0.425 | 1.309 | yes |
| +X / Worm servo output | 0.650 | 1.407 | yes |
| +X / Foot servo output | 0.664 | 1.542 | yes |
| +Y / Hip servo output | 0.515 | 1.185 | yes |
| +Y / Worm servo output | 0.728 | 1.435 | yes |
| +Y / Foot servo output | 0.837 | 2.040 | yes |
| -X / Hip servo output | 0.384 | 1.064 | yes |
| -X / Worm servo output | 0.116 | 0.302 | yes |
| -X / Foot servo output | 2.082 | 3.847 | no |

| Foot | Peak clearance (mm) | Lift excursions |
|---|---:|---:|
| -Y / Sliding foot crosshead | 4.76 | 4 |
| +X / Sliding foot crosshead | 11.50 | 4 |
| +Y / Sliding foot crosshead | 20.46 | 5 |
| -X / Sliding foot crosshead | 7.91 | 4 |

The diagnostic foot-joint errors have both a sustained bias and a varying component. That is consistent with load and dynamic tracking limitations, but does not identify a calibrated cause. Slowing the original waveform alone did not remove the error.

The bounded desired signal is part of this candidate, not the original ideal-motor gait. Comparison to ideal-motor speed is not a matched benchmark.

Further validation passed:

- Seven seconds walking, then three seconds stopping: maximum displacement after the stop request 5.92 mm; final-second drift 0.264 mm. No fall.
- Halving the physics timestep changed net travel by 0.313 mm, final body position by 0.394 mm, and the largest final actuated angle by 0.0458 degrees. All frozen numerical gates passed.
- Exact all-frame replay was checked on the three-second bounded baseline; a ten-second replay of this candidate has not been run.

These checks do not override the failed tracking gate. The recipe remains experimental and has not replaced the browser or FPGA controller.
