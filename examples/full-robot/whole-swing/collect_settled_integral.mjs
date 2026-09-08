import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const planPath = `${root}/settled-integral-numeric-plan.json`, plan = read(planPath);
const statusPath = `${root}/settled-integral-numeric-status.json`, status = read(statusPath);
const integrityPath = `${root}/settled-integral-numeric-integrity.json`, integrity = read(integrityPath);
assert(status.complete && integrity.passed);
for (const report of [plan, status, integrity]) report.sources.forEach(verify);
verify(plan.previous_capture); const old = read(plan.previous_capture.path);
const frames = r => r.frames.map(({stepping_wall_s, ...f}) => f);
const originalFrames = frames(old), prefix = originalFrames.filter(f => f.time_s <= 26);
const cases = status.cases.map((c, i) => {
  c.sources.forEach(verify); assert.equal(c.name, plan.cases[i].name);
  const capture = c.sources.find(s => s.path.endsWith('.native.json')), r = read(capture.path);
  assert.deepEqual(frames(r).slice(0, prefix.length), prefix);
  assert.deepEqual(r.recording.config, old.recording.config);
  assert.deepEqual(r.recording.scene.robot, old.recording.scene.robot);
  assert.deepEqual(r.recording.scene.options, old.recording.scene.options);
  assert.deepEqual(r.recording.input_events, old.recording.input_events);
  const gain = plan.cases[i].integral_gain_per_s;
  if (gain === 0) { assert.deepEqual(frames(r), originalFrames); assert.deepEqual(r.transitions, old.transitions); }
  const metricsPath = `${root}/settled-integral-${c.name}-metrics.json`;
  execFileSync(process.execPath, ['examples/interactive/analyze_walking_capture.mjs', capture.path,
    metricsPath, ...(!r.completed ? ['--accepted-prefix'] : [])], {stdio: 'pipe'});
  const m = read(metricsPath), afterStop = r.frames.filter(f => f.policy?.time_s >= 26);
  let maximumJump = 0, variation = 0, reversals = 0, previousDelta;
  for (let i = 1; i < afterStop.length; i++) {
    const a = afterStop[i - 1].policy.targets, b = afterStop[i].policy.targets;
    const delta = Object.keys(a).map(k => b[k] - a[k]);
    maximumJump = Math.max(maximumJump, ...delta.map(Math.abs));
    variation += delta.reduce((s, d) => s + Math.abs(d), 0);
    if (previousDelta) reversals += delta.filter((d, j) => Math.abs(d) > 1e-5 && Math.abs(previousDelta[j]) > 1e-5 && d * previousDelta[j] < 0).length;
    previousDelta = delta;
  }
  return {name: c.name, integral_gain_per_s: gain, completed: c.completed,
    task_passed: c.passed, error: c.error, acceptance: c.acceptance,
    pre_stop_frames_exact: prefix.length, full_gain_zero_reference_exact: gain === 0 ? true : null,
    after_stop_maximum_target_jump_rad: maximumJump, after_stop_target_variation_rad: variation,
    after_stop_target_delta_reversals_above_1e_minus5_rad: reversals,
    positive_mechanical_work_j: m.positive_mechanical_work_j,
    maximum_loaded_marker_path_m: Math.max(...m.feet.map(f => f.sampled_loaded_tangential_path_m)),
    native_compute: m.native_compute, metrics: source(metricsPath), capture};
});
writeFileSync(`${root}/settled-integral-summary.json`, JSON.stringify({version: 1, cases,
  focused_tests: {rust_kernel_and_registry: 4, rhai_binding_replay_and_numeric_boundary: 3},
  promoted: false,
  sources: [planPath, statusPath, integrityPath, import.meta.filename,
    'examples/interactive/analyze_walking_capture.mjs', 'examples/interactive/capture_outcome.mjs',
    'runs/angle-integral-final-tests.log', 'runs/angle-integral-numeric-tests.log'].map(source),
  scope: 'Same revealed development case and original gates. Gain-zero reference and every pre-stop frame match exactly, so the changed stop follows the bounded integral policy. Target reversals use consecutive deltas above 1e-5 rad. Loaded-marker travel is a geometric proxy. Passing this task alone does not establish minute, fresh held-out, terrain, numerical accuracy, browser performance or hardware qualification.'}, null, 2) + '\n');
console.log(JSON.stringify(cases.map(c => ({name: c.name, passed: c.task_passed,
  body_error_m: c.acceptance?.final_body_error_m, work_j: c.positive_mechanical_work_j,
  target_reversals: c.after_stop_target_delta_reversals_above_1e_minus5_rad,
  max_jump_rad: c.after_stop_maximum_target_jump_rad})), null, 2));
