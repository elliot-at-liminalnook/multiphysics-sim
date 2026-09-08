// Summarize the predeclared native study, retaining failed accepted prefixes.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import {captureOutcome} from '../../interactive/capture_outcome.mjs';
const root = 'examples/full-robot/whole-swing';
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const statusPath = `${root}/sdirk-steering-status.json`, status = read(statusPath);
const integrityPath = `${root}/sdirk-steering-integrity.json`, integrity = read(integrityPath);
const validationPath = `${root}/sdirk-validation-inputs.json`, validation = read(validationPath);
assert(status.complete && integrity.passed);
for (const report of [status, integrity, validation]) report.sources.forEach(verify);
assert.deepEqual(status.cases.map(c => c.name), integrity.outcomes.map(c => c.name));
const cases = status.cases.map((c, i) => {
  c.sources.forEach(verify);
  const capture = c.sources.find(s => s.path.endsWith('.native.json')); assert(capture);
  const r = read(capture.path), outcome = captureOutcome(r);
  assert.deepEqual(outcome, integrity.outcomes[i].outcome);
  const metricsPath = `${root}/sdirk-${c.name}-metrics.json`;
  execFileSync(process.execPath, ['examples/interactive/analyze_walking_capture.mjs',
    capture.path, metricsPath, ...(!r.completed ? ['--accepted-prefix'] : [])], {stdio: 'pipe'});
  const metrics = read(metricsPath), last = r.frames.at(-1);
  const maximum = field => r.frames.reduce((best, f) => {
    const value = Math.max(0, ...f.motor_readings.map(m => Math.abs(m[field])));
    return value > best.value ? {value, time_s: f.time_s} : best;
  }, {value: 0, time_s: 0});
  return {name: c.name, step_s: c.step_s, completed: c.completed, task_passed: c.passed,
    outcome, accepted_simulated_s: last.time_s, accepted_transitions: r.frames.length - 1,
    acceptance: c.acceptance, maximum_sampled_gear_speed_rad_s: maximum('gear_speed_rad_s'),
    maximum_sampled_shaft_torque_nm: maximum('shaft_torque_nm'),
    native_compute: metrics.native_compute, metrics: source(metricsPath), capture};
});
const comparisons = ['sdirk-student-refinement', 'sdirk-teacher-reference'].map(name => {
  const path = `${root}/${name}.json`, r = read(path);
  [r.baseline, r.candidate, r.markers].forEach(verify);
  const foot = r.metrics.foot_marker_position_m.maximum, body = r.metrics.body_position_m.maximum;
  return {name, source: source(path), maximum_foot_difference_m: foot,
    maximum_body_difference_m: body, foot_passed: foot <= .001, body_passed: body <= .0005,
    passed: foot <= .001 && body <= .0005};
});
const identityPath = `${root}/sdirk-default-identity.json`, identity = read(identityPath);
assert(identity.passed); identity.sources.forEach(verify);
const report = {version: 1, cases, comparisons,
  accuracy_budgets: {maximum_foot_difference_m: .001, maximum_body_difference_m: .0005},
  default_off_identity_passed: true, focused_rust_tests: validation.tests,
  browser_promoted: false, browser_tested: false,
  sources: [statusPath, integrityPath, validationPath, identityPath, import.meta.filename,
    'examples/interactive/capture_outcome.mjs', 'examples/interactive/analyze_walking_capture.mjs',
    'examples/full-robot/compare_mechanical_reuse.mjs'].map(source),
  scope: 'Fixed 24-second development steering, seed 0, unchanged task and trajectory budgets. Failed runs contain only accepted-prefix measurements, never full-episode qualification. All three coarse failures occur at 1.32 s; their immediate guard reasons do not establish the underlying numerical cause. Teacher and student retain different source solver tolerances/optimizations, so their timings are not controlled integrator comparisons. Native stepping excludes capture serialization and is not browser throughput. No new controller, terrain, robustness or sim-to-real qualification.'};
writeFileSync(`${root}/sdirk-steering-summary.json`, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({cases: cases.map(c => ({name: c.name, outcome: c.outcome.kind,
  task_passed: c.task_passed, simulated_s: c.accepted_simulated_s,
  native_rate: c.native_compute.simulation_per_wall})), comparisons}, null, 2));
