import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', run = 'runs/interactive/settled-integral';
const read = path => JSON.parse(readFileSync(path));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const statusPath = `${root}/settled-integral-browser-status.json`, status = read(statusPath);
const nativePath = `${root}/settled-integral-regression-status.json`, native = read(nativePath);
assert(status.complete && native.complete);
status.sources.forEach(verify);
assert(status.parity.passed && status.parity.replay_exact && status.parity.reset_exact);
const cases = status.cases.map(c => {
  c.sources.forEach(verify);
  const m = c.measurement;
  assert(m.completed && m.validation_passed && m.keyboard_commands_recorded);
  assert.equal(m.frame_encoding, 'json'); assert.equal(m.display_rate_hz, 0);
  assert.equal(m.runtime_build.browser_module_sha256, status.cases[0].measurement.runtime_build.browser_module_sha256);
  const recordPath = `${run}/${c.name}.recording.json`, recording = read(recordPath);
  assert.equal(recording.error, null);
  const n = native.cases.find(n => n.name === ({'teacher-turn': 'steering-1.25ms', 'teacher-minute': 'minute-1.25ms'})[c.name]);
  assert(n); n.sources.forEach(verify);
  const capture = n.sources.find(s => s.path.endsWith('.native.json'));
  assert.deepEqual(read(capture.path).recording, recording.runtime);
  return {name: c.name, exact_native_recipe_seed_and_inputs: true,
    native_task_passed: n.passed, acceptance: n.acceptance,
    realtime_passed: m.meets_speed_target && m.meets_transition_target,
    capture, browser_recording: source(recordPath)};
});
const uiPath = `${run}/leaderboard.json`, ui = read(uiPath); assert(ui.passed);
const report = {version: 1, passed: true, cases, ui,
  all_realtime_gates_passed: cases.every(c => c.realtime_passed),
  sources: [statusPath, nativePath, uiPath, import.meta.filename].map(source),
  scope: 'Rendered minute and steering recordings exactly match accepted native recipes, seeds and inputs. Steering native/WASM comparison and exact replay/reset pass; full-minute portability is not inferred. Every leaderboard load, tested steering replay, video export, mobile layout and tamper rejection is checked. Native task success and failed browser timing remain separate.'};
writeFileSync(`${root}/settled-integral-browser-integrity.json`, JSON.stringify(report, null, 2) + '\n');
console.log({passed: true, cases: cases.length, realtime: report.all_realtime_gates_passed});
