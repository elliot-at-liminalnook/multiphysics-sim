// Sequential measurements through the production rendered browser harness.
import {readFileSync, writeFileSync, existsSync, mkdirSync, openSync, closeSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import assert from 'node:assert/strict';
const [planPath, statusPath] = process.argv.slice(2);
assert(planPath && statusPath && !existsSync(statusPath), 'usage: measure_browser_suite.mjs plan.json fresh-status.json');
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const plan = read(planPath); plan.sources.forEach(verify); verify(plan.parity);
const parity = read(plan.parity.path); assert(parity.passed && parity.replay_exact && parity.reset_exact);
assert(plan.cases.length > 0 && new Set(plan.cases.map(c => c.name)).size === plan.cases.length);
assert(['object', 'json'].includes(plan.frame_encoding)); assert([0, 30].includes(plan.display_rate_hz));
const manifest = source(`${plan.bundle}/build-manifest.json`);
const sources = [planPath, plan.parity.path, import.meta.filename, 'web/tests/live_performance.mjs'].map(source).concat(manifest);
mkdirSync(plan.output_directory, {recursive: true});
const cases = [];
for (const c of plan.cases) {
  assert(/^[a-z0-9-]+$/.test(c.name)); sources.forEach(verify); plan.sources.forEach(verify);
  const report = `${plan.output_directory}/${c.name}.json`, log = `${plan.output_directory}/${c.name}.log`;
  assert(!existsSync(report) && !existsSync(log));
  const fd = openSync(log, 'wx');
  const result = spawnSync(process.execPath, ['web/tests/live_performance.mjs', plan.bundle, c.preset, report, c.scenario],
    {env: {...process.env, DISPLAY_RATE: String(plan.display_rate_hz), FRAME_ENCODING: plan.frame_encoding}, stdio: ['ignore', fd, fd]});
  closeSync(fd); assert(!result.error, result.error?.message); sources.forEach(verify); plan.sources.forEach(verify);
  const measurement = existsSync(report) ? read(report) : null;
  cases.push({...c, exit_code: result.status, signal: result.signal, measurement,
    sources: [log, report, report.replace('.json', '.recording.json'), report.replace('.json', '.timing.json')].filter(existsSync).map(source)});
  writeFileSync(statusPath, JSON.stringify({version: 1, complete: cases.length === plan.cases.length,
    cases, parity, sources, scope: plan.scope}, null, 2) + '\n');
  console.log({name: c.name, completed: measurement?.completed, active: measurement?.performance.active_motion,
    speed: measurement?.meets_speed_target, latency: measurement?.meets_transition_target});
}
