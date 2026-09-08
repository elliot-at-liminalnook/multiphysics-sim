import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const statusPath = `${root}/sdirk-tolerance-status.json`, status = read(statusPath);
assert(status.complete && status.cases.length === 2); status.sources.forEach(verify);
const planPath = `${root}/sdirk-tolerance-plan.json`, plan = read(planPath);
plan.sources.forEach(verify);
const captures = status.cases.map(c => { c.sources.forEach(verify);
  return read(c.sources.find(s => s.path.endsWith('.native.json')).path); });
const [authored, tight] = captures;
assert.deepEqual(authored.task, tight.task); assert.deepEqual(authored.recording.scene, tight.recording.scene);
assert.deepEqual(authored.recording.input_events, tight.recording.input_events);
const normalized = r => { const c = structuredClone(r.recording.config);
  delete c.implicit.newton.absolute_tolerance; delete c.implicit.newton.relative_tolerance; return c; };
assert.deepEqual(normalized(authored), normalized(tight));
assert.equal(authored.frames.length, tight.frames.length);
const previousPath = 'runs/full-robot/learning/sdirk-steering/teacher-sdirk2-20ms.native.json';
const previous = read(previousPath);
const physicsFrames = r => r.frames.map(({stepping_wall_s, ...f}) => f);
assert.deepEqual(physicsFrames(authored), physicsFrames(previous));
for (const key of ['transitions', 'recording', 'task', 'contract', 'completed', 'error'])
  assert.deepEqual(authored[key], previous[key], `instrumentation changes ${key}`);
const difference = {maximum_joint_position_rad: 0, maximum_joint_velocity_rad_s: 0, maximum_body_position_m: 0};
const bodyName = authored.recording.config.policy.body_feedback.reference_link;
for (let i = 0; i < authored.frames.length; i++) {
  const a = authored.frames[i], b = tight.frames[i]; assert.equal(a.time_s, b.time_s);
  for (const [field, metric] of [['joint_positions', 'maximum_joint_position_rad'], ['joint_velocities', 'maximum_joint_velocity_rad_s']]) {
    assert.equal(a[field].length, b[field].length);
    a[field].forEach((v, j) => { difference[metric] = Math.max(difference[metric], Math.abs(v - b[field][j])); });
  }
  const x = a.poses.find(p => p.name === bodyName), y = b.poses.find(p => p.name === bodyName);
  difference.maximum_body_position_m = Math.max(difference.maximum_body_position_m,
    Math.hypot(...x.position_m.map((v, j) => v - y.position_m[j])));
}
const cases = status.cases.map((c, i) => {
  const r = captures[i], p = read(c.sources.find(s => s.path.endsWith('.profile.json')).path);
  assert(!r.completed && r.error && r.frames.at(-1).time_s === 1.32);
  return {name: c.name, outcome: c.outcome,
    newton: r.recording.config.implicit.newton,
    accepted_transitions: c.accepted_transitions, accepted_simulated_s: c.accepted_simulated_s,
    samples: r.frames.filter(f => f.time_s >= 1.26).map(f => ({time_s: f.time_s,
      phase: f.policy.step_reference.reference.phase,
      body_speed_m_s: Math.hypot(...f.poses.find(p => p.name === bodyName).velocity_m_s),
      maximum_gear_speed_rad_s: Math.max(...f.motor_readings.map(m => Math.abs(m.gear_speed_rad_s))),
      external_force_world_n: f.environment_load.force_world_n})),
    solver_accepted_stage_records: p.accepted_implicit_steps.length,
    solver_accepted_interval_records: p.accepted_intervals.length,
    intervals_with_subdivision: p.accepted_intervals.filter(d => d.accepted_segments > 1).length,
    final_four_stage_residuals: p.accepted_implicit_steps.slice(-4).map(d => d.maximum_scaled_velocity_residual)};
});
writeFileSync(`${root}/sdirk-tolerance-summary.json`, JSON.stringify({version: 1, cases,
  authored_profile_preserves_physics_and_task_exactly: true, accepted_prefix_difference: difference,
  tightening_prevents_early_failure: false, browser_promoted: false,
  sources: [statusPath, planPath, previousPath, import.meta.filename].map(source),
  scope: 'Both tolerance choices retain the same early failure, so 1000x tighter absolute/relative Newton tolerances are not a sufficient remedy. This does not establish which integration, nonlinear contact or chart mechanism causes the jump. Profiles are instrumented; stage/interval records can include work before a failed environment transition and are not counts of published control frames. No task, trajectory, performance or hardware qualification.'}, null, 2) + '\n');
console.log(JSON.stringify({difference, cases: cases.map(c => ({name: c.name,
  last_sample: c.samples.at(-1), subdivisions: c.intervals_with_subdivision,
  final_four_stage_residuals: c.final_four_stage_residuals}))}, null, 2));
