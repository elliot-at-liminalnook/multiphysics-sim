// Sequential native diagnostics, including explicit failed/task-terminated runs.
import {readFileSync, writeFileSync, existsSync, openSync, closeSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {captureOutcome} from './capture_outcome.mjs';
const [planPath, outputDirectory, statusPath] = process.argv.slice(2);
assert(statusPath, 'usage: run_profiled_study plan.json existing-output-directory status.json');
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const plan = read(planPath), executable = source('target/release/examples/run_environment');
assert(Array.isArray(plan.cases) && plan.cases.length > 0);
const inputs = plan.cases.flatMap(c => [c.scene, c.config, c.task, c.actions].map(source));
const cases = [];
for (const c of plan.cases) {
  [...plan.sources, ...inputs, executable].forEach(verify);
  assert(/^[a-zA-Z0-9_-]+$/.test(c.name), 'invalid output name');
  const capture = `${outputDirectory}/${c.name}.native.json`, profile = c.profile,
    log = `${outputDirectory}/${c.name}.log`;
  assert(profile && !existsSync(capture) && !existsSync(profile) && !existsSync(log), 'refusing to overwrite diagnostics');
  const out = openSync(capture, 'wx'), err = openSync(log, 'wx');
  const execution = spawnSync(executable.path, [c.scene, c.config, c.task, c.actions, '--profile', profile],
    {stdio: ['ignore', out, err]});
  closeSync(out); closeSync(err);
  assert(!execution.error && [0, 1].includes(execution.status), `runner failed; inspect ${log}`);
  [...plan.sources, ...inputs, executable].forEach(verify);
  const r = read(capture), p = read(profile), outcome = captureOutcome(r);
  assert.equal(execution.status, outcome.kind === 'runtime_error' ? 1 : 0,
    'runner exit must match retained outcome');
  assert.equal(p.completed, r.completed);
  const steps = p.accepted_implicit_steps;
  cases.push({name: c.name, completed: r.completed, outcome,
    accepted_simulated_s: r.frames.at(-1).time_s, accepted_transitions: r.frames.length - 1,
    profiled_wall_s: p.wall_s, accepted_stage_solves: steps.length,
    maximum_accepted_scaled_velocity_residual: Math.max(0, ...steps.map(d => d.maximum_scaled_velocity_residual)),
    maximum_accepted_contact_history_residual: Math.max(0, ...steps.map(d => d.maximum_contact_history_residual)),
    buckets: p.buckets, sources: [c.scene, c.config, c.task, c.actions, capture, profile, log].map(source)});
  writeFileSync(statusPath, JSON.stringify({version: 1, complete: cases.length === plan.cases.length, cases,
    sources: [source(planPath), executable, source(import.meta.filename), source('examples/interactive/capture_outcome.mjs')],
    scope: `${plan.scope} Instrumented diagnostic only: completion means all cases were executed, not task acceptance. Accepted stage counters omit rejected outer attempts; profiler buckets can overlap. No browser timing claim.`}, null, 2) + '\n');
  console.log(JSON.stringify({name: c.name, outcome, simulated_s: r.frames.at(-1).time_s}));
}
