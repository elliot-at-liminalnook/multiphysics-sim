import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const planPath = `${root}/sdirk-physical-guess-plan.json`, plan = read(planPath);
const statusPath = `${root}/sdirk-physical-guess-status.json`, status = read(statusPath);
const integrityPath = `${root}/sdirk-physical-guess-integrity.json`, integrity = read(integrityPath);
assert(status.complete && integrity.passed);
for (const report of [plan, status, integrity]) report.sources.forEach(verify);
const cases = status.cases.map((c, i) => {
  c.sources.forEach(verify); const spec = plan.cases[i]; assert.equal(c.name, spec.name);
  verify(spec.previous_capture);
  const capture = c.sources.find(s => s.path.endsWith('.native.json')), r = read(capture.path);
  let default_identity = null;
  if (spec.integration === 'backward_euler') {
    const old = read(spec.previous_capture.path), frames = c => c.frames.map(({stepping_wall_s, ...f}) => f);
    assert.deepEqual(frames(r), frames(old));
    for (const key of ['transitions', 'recording', 'task', 'contract', 'completed', 'error']) assert.deepEqual(r[key], old[key]);
    default_identity = {passed: true, frames: r.frames.length, transitions: r.transitions.length,
      previous: spec.previous_capture, comparison: 'Strict parsed values including signed zero; only frame stepping_wall_s excluded.'};
  }
  const metricsPath = `${root}/sdirk-physical-guess-${c.name}-metrics.json`;
  execFileSync(process.execPath, ['examples/interactive/analyze_walking_capture.mjs', capture.path,
    metricsPath, ...(!r.completed ? ['--accepted-prefix'] : [])], {stdio: 'pipe'});
  return {name: c.name, completed: c.completed, task_passed: c.passed, error: c.error,
    accepted_simulated_s: r.frames.at(-1).time_s, acceptance: c.acceptance,
    default_identity, maximum_sampled_gear_speed_rad_s: Math.max(...r.frames.flatMap(f => f.motor_readings.map(m => Math.abs(m.gear_speed_rad_s)))),
    native_compute: read(metricsPath).native_compute, metrics: source(metricsPath), capture};
});
const comparisonPath = `${root}/sdirk-physical-guess-teacher-reference.json`, comparison = read(comparisonPath);
[comparison.baseline, comparison.candidate, comparison.markers].forEach(verify);
const foot = comparison.metrics.foot_marker_position_m.maximum, body = comparison.metrics.body_position_m.maximum;
writeFileSync(`${root}/sdirk-physical-guess-summary.json`, JSON.stringify({version: 1, cases,
  teacher_reference: {source: source(comparisonPath), maximum_foot_difference_m: foot,
    maximum_body_difference_m: body, foot_budget_m: .001, body_budget_m: .0005,
    passed: foot <= .001 && body <= .0005},
  browser_promoted: false,
  sources: [planPath, statusPath, integrityPath, import.meta.filename,
    'examples/interactive/analyze_walking_capture.mjs', 'examples/interactive/capture_outcome.mjs'].map(source),
  scope: 'Predeclared 24-second native steering screen. Same physical equations and tolerances; experimental SDIRK second-stage initial guess now uses the first stage velocity. Default backward Euler is checked exactly. Task checks and native rates are separate from missing trajectory refinement and browser qualification; failed prefixes are not full episodes.'}, null, 2) + '\n');
console.log(JSON.stringify(cases.map(c => ({name: c.name, passed: c.task_passed, error: c.error,
  time_s: c.accepted_simulated_s, max_gear_rad_s: c.maximum_sampled_gear_speed_rad_s,
  native_rate: c.native_compute.simulation_per_wall})), null, 2));
