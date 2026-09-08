// Measurements from recorded Rust physics; this script never advances dynamics.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {captureOutcome} from './capture_outcome.mjs';
const [capturePath, output] = process.argv.slice(2);
assert(capturePath && output, 'usage: analyze_walking_capture.mjs capture.json report.json');
const bytes = readFileSync(capturePath), c = JSON.parse(bytes);
const outcome = captureOutcome(c);
const acceptedPrefix = process.argv.includes('--accepted-prefix') && !c.completed;
assert(c.completed || acceptedPrefix && c.frames.length >= 2,
  'complete capture required unless explicitly measuring the accepted prefix of a retained failed run');
const config = c.recording.config, frames = c.frames;
const bodyName = config.policy.body_feedback.reference_link;
const markers = config.policy.point_feedback.markers;
const names = c.metadata.coordinate_names, indices = c.metadata.joint_indices;
const body = f => { const p = f.poses.find(p => p.name === bodyName); assert(p); return p; };
const sub = (a, b) => a.map((v, i) => v - b[i]);
const norm = a => Math.hypot(...a);
const dot = (a, b) => a.reduce((s, v, i) => s + v * b[i], 0);
const marker = (f, m) => {
  const p = f.poses.find(p => p.name === m.link); assert(p);
  return p.position_m.map((v, i) => v + dot(p.rotation[i], m.local_point_m));
};
const markerLinks = new Map(markers.map(m => {
  const index = c.recording.scene.robot.links.findIndex(l => l.name === m.link);
  assert(index >= 0, `missing CAD marker link ${m.link}`); return [m.id, index];
}));
const force = (f, m) => f.contacts.filter(p => p.link === markerLinks.get(m.id) && p.other == null)
  .reduce((s, p) => s + p.force_n[2], 0);
const phase = f => f.policy?.step_reference?.reference ?? {
  phase: 'initial', waiting: false, body_world_m: body(f).position_m};
const quantile = (a, q) => [...a].sort((x, y) => x - y)[Math.ceil(q * a.length) - 1];
const phases = {}, motors = names.map(name => ({name, maximum_tracking_error_rad: 0,
  maximum_reference_error_rad: 0, maximum_speed_rad_s: 0, maximum_torque_nm: 0,
  positive_mechanical_work_j: 0, negative_mechanical_work_j: 0}));
const feet = markers.map(m => ({id: m.id, sampled_loaded_tangential_path_m: 0, maximum_loaded_tangential_speed_m_s: 0}));
let horizontalPath = 0;
for (let i = 1; i < frames.length; i++) {
  const a = frames[i - 1], b = frames[i], dt = b.time_s - a.time_s; assert(dt > 0);
  const r = phase(a), displacement = sub(body(b).position_m, body(a).position_m);
  const p = phases[r.phase] ??= {duration_s: 0, waiting_s: 0, body_displacement_world_m: [0, 0, 0],
    horizontal_body_path_m: 0, planned_body_path_m: 0};
  p.duration_s += dt; if (r.waiting) p.waiting_s += dt;
  displacement.forEach((v, j) => p.body_displacement_world_m[j] += v);
  p.horizontal_body_path_m += Math.hypot(...displacement.slice(0, 2));
  p.planned_body_path_m += norm(sub(phase(b).body_world_m, r.body_world_m));
  horizontalPath += Math.hypot(...displacement.slice(0, 2));
  markers.forEach((m, j) => {
    // Only intervals whose endpoints are loaded. This is sampled tangential
    // motion of a geometric marker, not a contact-patch or between-sample proof.
    if (force(a, m) >= 1 && force(b, m) >= 1) {
      const d = norm(sub(marker(b, m), marker(a, m)).slice(0, 2));
      feet[j].sampled_loaded_tangential_path_m += d;
      feet[j].maximum_loaded_tangential_speed_m_s = Math.max(feet[j].maximum_loaded_tangential_speed_m_s, d / dt);
    }
  });
  motors.forEach((m, j) => {
    for (const f of [a, b]) {
      m.maximum_tracking_error_rad = Math.max(m.maximum_tracking_error_rad, Math.abs(f.servo_targets_rad[j] - f.joint_positions[indices[j]]));
      m.maximum_reference_error_rad = Math.max(m.maximum_reference_error_rad, Math.abs(f.reference_targets_rad[j] - f.joint_positions[indices[j]]));
      const reading = f.motor_readings[j];
      m.maximum_speed_rad_s = Math.max(m.maximum_speed_rad_s, Math.abs(reading.gear_speed_rad_s));
      m.maximum_torque_nm = Math.max(m.maximum_torque_nm, Math.abs(reading.shaft_torque_nm));
      const power = reading.gear_speed_rad_s * reading.shaft_torque_nm;
      m.positive_mechanical_work_j += Math.max(0, power) * dt / 2;
      m.negative_mechanical_work_j += Math.min(0, power) * dt / 2;
    }
  });
}
const channels = config.policy.step_reference.command_channels;
const commandIndices = channels.map(name => c.recording.scene.controller.inputs.findIndex(i => i.name === name));
assert(commandIndices.every(i => i >= 0));
const command = f => commandIndices.map(i => f.policy_inputs[i]);
const windows = [];
let start = 0;
for (let i = 1; i <= frames.length; i++) {
  if (i < frames.length && command(frames[i]).every((v, j) => v === command(frames[start])[j])) continue;
  const cmd = command(frames[start]), end = frames[i - 1];
  // Report sustained straight travel only, after four seconds in this command.
  // This window is a measurement convention, not a controller acceptance gate.
  const selected = frames.slice(start, i).filter(f => f.time_s >= frames[start].time_s + 4);
  if (cmd[2] === 0 && Math.hypot(cmd[0], cmd[1]) > 0 && selected.length >= 2) {
    const initial = body(selected[0]), yaw = Math.atan2(initial.rotation[1][0], initial.rotation[0][0]);
    const speed = Math.hypot(cmd[0], cmd[1]);
    const axis = [(Math.cos(yaw) * cmd[0] - Math.sin(yaw) * cmd[1]) / speed,
      (Math.sin(yaw) * cmd[0] + Math.cos(yaw) * cmd[1]) / speed, 0];
    const times = selected.map(f => f.time_s - selected[0].time_s);
    const positions = selected.map(f => dot(sub(body(f).position_m, initial.position_m), axis));
    const mean = a => a.reduce((s, v) => s + v, 0) / a.length;
    const mt = mean(times), mp = mean(positions);
    const slope = times.reduce((s, t, j) => s + (t - mt) * (positions[j] - mp), 0) /
      times.reduce((s, t) => s + (t - mt) ** 2, 0);
    windows.push({command_start_s: frames[start].time_s, start_s: selected[0].time_s, end_s: end.time_s,
      command: cmd, samples: selected.length, measured_sustained_speed_m_s: slope,
      net_projected_speed_m_s: positions.at(-1) / times.at(-1), axis_world: axis});
  }
  start = i;
}
const delta = sub(body(frames.at(-1)).position_m, body(frames[0]).position_m);
const work = motors.reduce((s, m) => s + m.positive_mechanical_work_j, 0);
writeFileSync(output, JSON.stringify({version: 1,
  ...(acceptedPrefix ? {accepted_prefix_only: true, episode_completed: false, episode_error: c.error, outcome,
    failure_scope: 'Only accepted frames before termination are measured. No full-episode task, speed, energy or recovery qualification is implied.'} : {}),
  capture: {path: capturePath, sha256: createHash('sha256').update(bytes).digest('hex')},
  source: {path: 'examples/interactive/analyze_walking_capture.mjs', sha256: createHash('sha256').update(readFileSync(import.meta.filename)).digest('hex')},
  outcome_source: {path: 'examples/interactive/capture_outcome.mjs', sha256: createHash('sha256').update(readFileSync(new URL('./capture_outcome.mjs', import.meta.url))).digest('hex')},
  simulated_s: frames.at(-1).time_s, body_displacement_world_m: delta,
  horizontal_body_path_m: horizontalPath, net_horizontal_displacement_m: Math.hypot(...delta.slice(0, 2)),
  phases, sustained_windows: windows, feet, motors,
  positive_mechanical_work_j: work, positive_mechanical_work_per_net_horizontal_m: work / Math.hypot(...delta.slice(0, 2)),
  native_compute: {wall_s: c.wall_s, simulation_per_wall: frames.at(-1).time_s / c.wall_s,
    transition_p95_s: quantile(c.transition_wall_s, .95)},
  conventions: {steady_command_warmup_s: 4, loaded_marker_endpoint_force_n: 1,
    work_integration: 'Trapezoidal samples of shaft torque times gear speed, positive/negative separated; no electrical efficiency or between-sample peak claim.'},
  scope: 'Measurements from sampled shared Rust physics. Body travel is distinct from path length, commands and native compute throughput. Loaded-marker motion is a slip diagnostic, not a contact-patch certificate. No acceptance, calibration, browser-performance or hardware-energy claim.'}, null, 2) + '\n');
