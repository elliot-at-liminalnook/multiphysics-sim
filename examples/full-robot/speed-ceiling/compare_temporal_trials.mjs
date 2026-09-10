// Compare recorded measurements, without replaying or approximating physics.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';

const directory = 'examples/full-robot/speed-ceiling';
const read = path => JSON.parse(fs.readFileSync(path));
const hash = path => crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex');
const summary = read(`${directory}/validation-summary.json`);
assert(process.argv.length===2||process.argv.length===5,'usage: compare_temporal_trials.mjs [family reference-name output-stem]');
const family=process.argv[2]??'flat125', outputStem=process.argv[4]??'temporal-finest-comparison';
assert(/^[a-z0-9-]+$/.test(outputStem),'safe output stem required');
const referenceName = process.argv[3]??read(`${directory}/temporal-solver-trials.json`).accuracy_reference;
const definitions = summary.rows.filter(row => row.family === family && row.kind === 'human');
const referenceRow = definitions.find(row => row.name === referenceName);
assert(referenceRow?.completed, 'finest declared reference must have completed');
assert.equal(referenceRow.step_s,Math.min(...definitions.filter(row=>row.completed).map(row=>row.step_s)),'reference must use the finest completed family step');
const reference = read(`${referenceRow.prefix}.native.json`);
const normDifference = (a, b) => Math.hypot(...a.map((value, i) => value - b[i]));
const chassis = frame => frame.poses.find(pose => pose.name.includes('Chassis'));
const markerPoint = (frame, marker) => {
  const pose = frame.poses.find(pose => pose.name === marker.link);
  assert(pose, `missing marker link ${marker.link}`);
  return pose.position_m.map((value, i) => value + pose.rotation[i].reduce((sum, r, j) => sum + r * marker.local_point_m[j], 0));
};
const physicalConfig = config => {
  const copy = structuredClone(config);
  for (const key of ['step_s', 'steps', 'report_every']) delete copy[key];
  delete copy.implicit.newton.broyden_updates;
  for (const key of ['linearized_jacobian_probes', 'linearized_probe_relative_step', 'reuse_exact_probe_base', 'sdirk2']) delete copy.implicit[key];
  return copy;
};
const rows = [];
for (const definition of definitions.filter(row => row.name !== referenceName)) {
  const path = `${definition.prefix}.native.json`, candidate = read(path);
  assert(candidate.completed, `${definition.name} incomplete`);
  for (const field of ['scene', 'seed']) {
    assert(field in candidate.recording && field in reference.recording, `missing recording.${field}`);
    assert.deepEqual(candidate.recording[field], reference.recording[field], `recording.${field}`);
  }
  const events = candidate.recording.input_events, referenceEvents = reference.recording.input_events;
  assert(Array.isArray(events) && Array.isArray(referenceEvents), 'missing recorded inputs');
  assert.equal(events.length, referenceEvents.length);
  for (let i = 0; i < events.length; ++i) {
    assert.deepEqual(events[i].values, referenceEvents[i].values, 'input values changed');
    assert(Math.abs(events[i].at_step * candidate.recording.config.step_s - referenceEvents[i].at_step * reference.recording.config.step_s) < 1e-9, 'input simulation time changed');
  }
  assert.deepEqual(candidate.task, reference.task, 'task changed');
  assert.deepEqual(physicalConfig(candidate.recording.config), physicalConfig(reference.recording.config), 'non-numerical config changed');
  assert.equal(candidate.frames.length, reference.frames.length);
  let maxBody = 0, bodySquared = 0, maxVelocity = 0, maxRotation = 0;
  const feet = reference.recording.config.policy.task_observations.markers.map(marker => ({marker, maximum_world_point_difference_m: 0}));
  for (let i = 0; i < reference.frames.length; ++i) {
    const a = reference.frames[i], b = candidate.frames[i];
    assert(Math.abs(a.time_s - b.time_s) < 1e-9, 'sample time mismatch');
    const pa = chassis(a), pb = chassis(b), difference = normDifference(pa.position_m, pb.position_m);
    maxBody = Math.max(maxBody, difference); bodySquared += difference * difference;
    maxVelocity = Math.max(maxVelocity, normDifference(pa.velocity_m_s, pb.velocity_m_s));
    const trace = pa.rotation.flat().reduce((sum, value, j) => sum + value * pb.rotation.flat()[j], 0);
    maxRotation = Math.max(maxRotation, Math.acos(Math.max(-1, Math.min(1, (trace - 1) / 2))));
    for (const foot of feet) foot.maximum_world_point_difference_m = Math.max(foot.maximum_world_point_difference_m, normDifference(markerPoint(a, foot.marker), markerPoint(b, foot.marker)));
  }
  const speedFraction = Math.max(...referenceRow.windows.map((window, i) => Math.abs(window.speed_m_s - definition.windows[i].speed_m_s) / window.speed_m_s));
  rows.push({name: definition.name, capture_sha256: hash(path), simulated_s: candidate.frames.at(-1).time_s,
    maximum_body_difference_m: maxBody, rms_body_difference_m: Math.sqrt(bodySquared / reference.frames.length),
    maximum_body_velocity_difference_m_s: maxVelocity, maximum_body_rotation_difference_rad: maxRotation,
    feet, maximum_steady_speed_fraction: speedFraction,
    absolute_slip_ratio_difference: Math.abs(referenceRow.maximum_slip_ratio - definition.maximum_slip_ratio),
    maximum_slip_ratio: definition.maximum_slip_ratio, turn_rad: definition.turn_rad,
    stops: definition.stops, passed_control_checks: definition.passed_control_checks,
    passed_contact_quality: definition.passed_contact_quality,
    passed_existing_path_speed_screen: maxBody <= .003 && speedFraction <= .02});
}
const result = {reference: referenceName, reference_capture_sha256: hash(`${referenceRow.prefix}.native.json`),
  sampled_s: .02, rows,
  family,reference_step_s:referenceRow.step_s,
  scope: 'Human numerical profiles against the declared finest completed family timestep. Scene, task, seed, inputs, controller, actuator/contact model and convergence tolerances must match. Existing screen: <=3 mm maximum body path difference and <=2% steady speed difference. Foot, orientation, contact quality and stop metrics remain explicit; this screen alone does not qualify physical accuracy or realtime performance. The reference is a numerical approximation, not ground truth; no between-sample guarantee.'};
fs.writeFileSync(`${directory}/${outputStem}.json`, JSON.stringify(result, null, 2) + '\n');
console.log(rows.map(({name, maximum_body_difference_m, maximum_steady_speed_fraction, maximum_slip_ratio, passed_existing_path_speed_screen}) => ({name, maximum_body_difference_m, maximum_steady_speed_fraction, maximum_slip_ratio, passed_existing_path_speed_screen})));
