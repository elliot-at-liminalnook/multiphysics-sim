// Read-only directional travel checks over shared Rust simulation captures.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
const [mode, input, baseline, protocol, output] = process.argv.slice(2);
assert(output, 'Usage: check_contact_direction.mjs forward|reverse CAPTURE BASELINE_CAPTURE QUALIFICATION_PROTOCOL NEW_REPORT');
assert(['forward', 'reverse'].includes(mode));
const read = p => JSON.parse(fs.readFileSync(p));
const hash = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const c = read(input), b = read(baseline), p = read(protocol);
assert.equal(c.kind, 'sampled_environment_capture');
assert.equal(c.recording.scene.robot.source.cad_sha256, b.recording.scene.robot.source.cad_sha256);
assert.equal(c.recording.seed, b.recording.seed);
const direction = p.direction_world_xy;
assert(Math.abs(Math.hypot(...direction) - 1) < 1e-12);
const projection = xy => xy.reduce((sum, x, i) => sum + x * direction[i], 0);
const last = c.transitions.at(-1), bLast = b.transitions.at(-1);
const forward = c.contract.actions.findIndex(x => x.name === 'command.forward_speed');
assert(forward >= 0);
const metrics = {};
const checks = {completed: c.completed && !c.error && !c.recording.failure,
  not_fallen: c.transitions.every(t => !t.speed.fallen)};
if (mode === 'forward') {
  metrics.signed_forward_distance_m = projection(last.speed.displacement_xy_m);
  metrics.signed_forward_speed_m_s = metrics.signed_forward_distance_m / last.time_s;
  metrics.forward_alignment = metrics.signed_forward_distance_m / last.speed.net_distance_m;
  metrics.matched_baseline_forward_speed_m_s = projection(bLast.speed.displacement_xy_m) / bLast.time_s;
  metrics.speed_ratio_to_matched_baseline = metrics.signed_forward_speed_m_s / metrics.matched_baseline_forward_speed_m_s;
  checks.horizon = p.forward_horizons_s.some(t => Math.abs(t - last.time_s) < 1e-8);
  checks.forward_command = c.frames.slice(1).every(f => f.policy_inputs[forward] > 0);
  checks.forward_speed = metrics.signed_forward_speed_m_s >= p.minimum_forward_speed_m_s;
  checks.alignment = metrics.forward_alignment >= p.minimum_forward_alignment;
  checks.speed_ratio = metrics.speed_ratio_to_matched_baseline >= p.minimum_speed_ratio_to_matched_baseline;
} else {
  const r = p.reverse;
  const begin = c.transitions.findIndex(t => t.time_s >= r.settled_window_start_s);
  assert(begin >= 0);
  const start = c.transitions[begin];
  assert(Math.abs(start.time_s - r.settled_window_start_s) < 1e-8);
  metrics.signed_reverse_travel_m = -projection(last.speed.displacement_xy_m.map((x, i) => x - start.speed.displacement_xy_m[i]));
  metrics.minimum_body_up_z = Math.min(...c.transitions.map(t => t.speed.body_up_z));
  // Include the change-of-direction transient in tracking, not only the settled tail.
  const trackingBegin = c.transitions.findIndex(t => t.time_s >= r.reverse_request_s);
  metrics.motors = c.recording.config.motors.target_coordinates.map((coordinate, k) => {
    const index = c.task.observations.findIndex(o => o.source.kind === 'coordinate_position' && o.source.coordinate === coordinate);
    assert(index >= 0);
    const errors = c.frames.slice(trackingBegin).map((f, i) => (f.servo_targets_rad[k] - c.transitions[i + trackingBegin].observations[index]) * 180 / Math.PI);
    return {coordinate, rms_degrees: Math.sqrt(errors.reduce((s, x) => s + x * x, 0) / errors.length), peak_degrees: Math.max(...errors.map(Math.abs))};
  });
  checks.horizon = Math.abs(last.time_s - r.horizon_s) < 1e-8;
  checks.forward_then_reverse_command = c.frames.slice(1).every(f => f.time_s <= r.reverse_request_s + 1e-8 ? f.policy_inputs[forward] > 0 : f.policy_inputs[forward] < 0);
  checks.reverse_travel = metrics.signed_reverse_travel_m >= r.minimum_signed_reverse_travel_m;
  checks.upright = metrics.minimum_body_up_z >= r.minimum_body_up_z;
  checks.tracking = metrics.motors.every(m => m.rms_degrees <= r.maximum_each_motor_rms_degrees && m.peak_degrees <= r.maximum_each_motor_peak_degrees);
}
const report = {mode, simulation_only: true, inputs: [input, baseline, protocol].map(path => ({path, sha256: hash(path)})), metrics, checks, pass: Object.values(checks).every(Boolean)};
fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify(report));
