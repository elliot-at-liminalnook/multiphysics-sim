# Direct body transfer between support postures

The shared Rust `StepSequenceConfig.direct_support_transfer` option defaults to
false. When enabled, the body reference holds its end-of-swing position and yaw
after landing, through the existing return/settle intervals. The next shift
starts at that held reference and moves directly to the next support posture.
The logical commanded center, foot-landing plan and readiness guards remain
separate from that held body reference.

For example, a sequence with phase durations `[0.58, 0.38, 0.38, 0.02, 0.02]`
retains a 1.38-second nominal transfer while devoting most of its all-feet-down
time to one smooth support-to-support shift. Durations remain explicit and must
lie on the controller sample grid. They are policy parameters, not changes to
robot inertia, friction, motor strength or physics time.

A stop completes an airborne landing. After its transfer is counted, the
sequence recenters from the held body reference over the configured shift time,
then allows the configured settle time and requires landing readiness. A stop
before lift, when `update_command_before_lift` is enabled, recenters without
counting a transfer. Resumption starts from the recentered pose. Readiness
failures remain bounded and transactional.

The runtime still checks CAD inverse kinematics and planned support at each
sample, and its ordinary actuator/contact dynamics determine actual motion.
Reference continuity is not a contact or balance guarantee. The focused tests
cover unchanged foot landings under turning commands, held position/yaw across
phase boundaries, cancellation and airborne stops, recenter/resume, readiness
rollback, and default serialization. Robot-level speed, sliding, accuracy and
browser cost require separate recorded experiments.
