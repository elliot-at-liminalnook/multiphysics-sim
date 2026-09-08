import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import {captureOutcome} from '../../interactive/capture_outcome.mjs';
const root = 'examples/full-robot/whole-swing';
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const [planPath = `${root}/settled-integral-regression-plan.json`,
  statusPath = `${root}/settled-integral-regression-status.json`,
  integrityPath = `${root}/settled-integral-regression-integrity.json`,
  outputPrefix = `${root}/settled-integral-regression`] = process.argv.slice(2);
const plan = read(planPath), status = read(statusPath), integrity = read(integrityPath);
assert(status.complete && integrity.passed);
for (const report of [plan, status, integrity]) report.sources.forEach(verify);
assert.equal(status.cases.length, plan.cases.length);
const budgets = {maximum_body_error_m: .001, maximum_yaw_error_rad: .005,
  maximum_foot_difference_m: .001, maximum_body_difference_m: .0005};
const cases = status.cases.map((c, i) => {
  assert.equal(c.name, plan.cases[i].name); c.sources.forEach(verify);
  const capture = c.sources.find(s => s.path.endsWith('.native.json')), r = read(capture.path);
  const outcome = captureOutcome(r), split = plan.cases[i].split;
  const metricsPath = `${outputPrefix}-${c.name}-metrics.json`;
  let m = null;
  if (r.completed || r.frames.length >= 2) {
    execFileSync(process.execPath, ['examples/interactive/analyze_walking_capture.mjs', capture.path,
      metricsPath, ...(!r.completed ? ['--accepted-prefix'] : [])], {stdio: 'pipe'});
    m = read(metricsPath);
  }
  const stops = split !== 'heldout' ? [] : plan.intermediate_stop_check_times_s.map(time => {
    const frame = r.frames.find(f => Math.abs(f.time_s - time) < 1e-8);
    const transition = r.transitions.find(t => Math.abs(t.time_s - time) < 1e-8);
    if (!frame || !transition) return {time_s: time, observed: false, passed: false};
    const reference = frame.policy.step_reference.reference, walking = transition.walking;
    assert.equal(reference.sample, walking.reference_sample);
    const pose = frame.poses.find(p => p.name === r.recording.config.policy.body_feedback.reference_link);
    const bodyError = Math.hypot(...pose.position_m.map((v, j) => v - reference.body_world_m[j]));
    assert(Math.abs(bodyError - Math.hypot(...walking.body_error_world_m)) < 1e-12);
    const yawError = Math.abs(walking.heading.error_rad);
    return {time_s: time, policy_time_s: frame.policy.time_s, observed: true,
      reference_phase: reference.phase, body_error_m: bodyError, yaw_error_rad: yawError,
      passed: bodyError <= budgets.maximum_body_error_m && yawError <= budgets.maximum_yaw_error_rad && reference.phase === 'idle'};
  });
  return {name: c.name, split, completed: c.completed, error: c.error, outcome,
    task_passed: c.passed, acceptance: c.acceptance, stop_checks: stops,
    declared_case_passed: c.passed && stops.every(s => s.passed),
    sustained_windows: m?.sustained_windows ?? [], positive_mechanical_work_j: m?.positive_mechanical_work_j ?? null,
    maximum_loaded_marker_path_m: m ? Math.max(...m.feet.map(f => f.sampled_loaded_tangential_path_m)) : null,
    native_compute: m?.native_compute ?? null, metrics: m ? source(metricsPath) : null, capture};
});
const a = cases.find(c => c.name === 'minute-1.25ms'), b = cases.find(c => c.name === 'minute-0.625ms');
let comparison = {observed: false, passed: false, reason: 'Both minute captures must complete.'};
if (a.completed && b.completed) {
  const path = `${outputPrefix}-refinement.json`;
  execFileSync(process.execPath, ['examples/full-robot/compare_mechanical_reuse.mjs',
    a.capture.path, b.capture.path, path, '--timestep-reference'], {stdio: 'pipe'});
  const r = read(path), foot = r.metrics.foot_marker_position_m.maximum, body = r.metrics.body_position_m.maximum;
  comparison = {observed: true, maximum_foot_difference_m: foot, maximum_body_difference_m: body,
    passed: foot <= budgets.maximum_foot_difference_m && body <= budgets.maximum_body_difference_m,
    source: source(path)};
}
const report = {version: 1, cases, comparison, budgets,
  declared_suite_passed: cases.every(c => c.declared_case_passed) && comparison.passed,
  browser_promoted: false,
  sources: [planPath, statusPath, integrityPath, import.meta.filename,
    'examples/interactive/analyze_walking_capture.mjs', 'examples/interactive/capture_outcome.mjs',
    'examples/full-robot/compare_mechanical_reuse.mjs'].map(source),
  scope: 'Frozen development, timestep refinement and two fresh held-out command cases. Intermediate stop checks pair each recorded physical endpoint with its own held reference and task transition; policy time precedes the physical endpoint by one control interval. Accepted failed prefixes do not qualify full episodes. Numerical budgets compare finite timesteps, not ground truth. No terrain, browser performance, hardware calibration or sim-to-real qualification.'};
writeFileSync(`${outputPrefix}-summary.json`, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({cases: cases.map(c => ({name: c.name, passed: c.declared_case_passed,
  task_passed: c.task_passed, error: c.error, final_body_error_m: c.acceptance?.final_body_error_m,
  stops: c.stop_checks, speeds: c.sustained_windows.map(w => w.measured_sustained_speed_m_s)})), comparison}, null, 2));
