import {existsSync, openSync, closeSync, readFileSync, writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', run = 'runs/interactive/settled-integral', bundle = `${run}/viewer`;
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const parityPath = `${root}/settled-integral-browser-parity.json`, parity = read(parityPath);
assert(parity.passed && parity.replay_exact && parity.reset_exact);
const manifest = source(`${bundle}/build-manifest.json`), cases = [];
for (const [name, preset, scenario] of [
  ['teacher-turn', 'tested-integral-teacher-steering', 'turn-reverse'],
  ['teacher-minute', 'tested-integral-teacher-minute', 'sustained-forward'],
]) {
  const report = `${run}/${name}.json`, log = `${run}/${name}.log`;
  assert(!existsSync(report) && !existsSync(log));
  assert.deepEqual(source(manifest.path), manifest);
  const fd = openSync(log, 'wx');
  const result = spawnSync(process.execPath, ['web/tests/live_performance.mjs', bundle, preset, report, scenario],
    {env: {...process.env, DISPLAY_RATE: '0', FRAME_ENCODING: 'json'}, stdio: ['ignore', fd, fd]});
  closeSync(fd); assert(!result.error, result.error?.message);
  const measurement = existsSync(report) ? read(report) : null;
  cases.push({name, preset, scenario, exit_code: result.status, measurement,
    sources: [log, report, report.replace('.json', '.recording.json'), report.replace('.json', '.timing.json')]
      .filter(existsSync).map(source)});
  assert.deepEqual(source(manifest.path), manifest);
  writeFileSync(`${root}/settled-integral-browser-status.json`, JSON.stringify({version: 1,
    complete: cases.length === 2, cases, parity,
    sources: [parityPath, 'web/tests/live_performance.mjs', import.meta.filename].map(source).concat(manifest),
    scope: 'Sequential rendered steering and sustained minute for the frozen integral teacher. Same isolated WASM, automatic rendering, JSON frames and 50 Hz control. Fixed >=1 active/overall pace and <=20 ms p95 gates. Exact recording/native identity and physical acceptance are checked separately; timing failures remain retained.'}, null, 2) + '\n');
  console.log({name, completed: measurement?.completed, active: measurement?.performance.active_motion,
    speed: measurement?.meets_speed_target, latency: measurement?.meets_transition_target});
}
