// Sequential measured executions; checkpoint each completed or failed case.
import {readFileSync, writeFileSync, openSync, closeSync, existsSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const run = process.argv[2] ?? 'runs/full-robot/learning/hybrid-speed';
const reportPath = process.argv[3] ?? 'examples/full-robot/hybrid-speed/status.json';
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const plan = read(`${run}/plan.json`);
for (const s of plan.sources) assert.equal(source(s.path).sha256, s.sha256);
const results = [];
for (const c of plan.cases) {
  const path = `${run}/${c.name}.native.json`;
  assert(!existsSync(path), `refusing to overwrite an experiment: ${path}`);
  const fd = openSync(path, 'wx'), log = openSync(`${run}/${c.name}.log`, 'wx');
  const execution = spawnSync('target/release/examples/run_environment', [c.scene, c.config, c.task, c.actions], {stdio: ['ignore', fd, log]});
  closeSync(fd); closeSync(log); assert(!execution.error, execution.error?.message);
  const capture = read(path);
  let acceptance = null;
  if (capture.completed && !capture.error) {
    const log = openSync(`${run}/${c.name}-acceptance.log`, 'wx');
    const check = spawnSync(process.execPath, ['examples/full-robot/check_online_steps.mjs', path, `${run}/${c.name}-acceptance`], {stdio: ['ignore', log, log]});
    closeSync(log); assert(!check.error, check.error?.message);
    const a = read(`${run}/${c.name}-acceptance/summary.json`);
    assert.equal(a.capture.sha256, source(path).sha256);
    acceptance = {passed: a.passed, lifts: a.lifts.length, failed_lifts: a.lifts.filter(l => !l.passed).length,
      final_body_error_m: a.final_body_error_m, final_yaw_error_rad: a.final_yaw_error_rad,
      maximum_body_tilt_rad: a.maximum_body_tilt_rad, sampled_internal_contacts: a.sampled_internal_contacts,
      body_advance_world_m: a.body_advance_world_m, budgets: a.budgets,
      source: source(`${run}/${c.name}-acceptance/summary.json`)};
  }
  const r = {name: c.name, gain: c.gain, duration_s: c.duration_s, step_s: c.step_s,
    completed: capture.completed, error: capture.error, passed: acceptance?.passed ?? false, acceptance,
    wall_s: capture.wall_s, sources: [c.scene, c.config, c.task, c.actions, path].map(source)};
  results.push(r);
  writeFileSync(reportPath, JSON.stringify({version: 1,
    complete: results.length === plan.cases.length, cases: results,
    sources: [source(`${run}/plan.json`), source(import.meta.filename), source('target/release/examples/run_environment'),
      source('examples/full-robot/check_online_steps.mjs'), source('target/release/examples/evaluate_lift')],
    scope: plan.scope}, null, 2) + '\n');
  console.log(JSON.stringify({name: c.name, passed: r.passed, error: r.error, acceptance}));
}
