import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', run = 'runs/interactive/broyden';
const read = p => JSON.parse(readFileSync(p)), source = path => ({path, sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const names = ['reference-turn', 'secant-turn', 'secant-forward'];
const episodes = Object.fromEntries(names.map(name => [name, read(`${run}/${name}.json`)]));
const recordings = Object.fromEntries(names.map(name => [name, read(`${run}/${name}.recording.json`)]));
for (const name of names) {
  assert(episodes[name].completed && episodes[name].validation_passed && episodes[name].keyboard_commands_recorded);
  assert.equal(recordings[name].error, null); assert.equal(recordings[name].runtime.seed, 0);
  assert.equal(episodes[name].runtime_build.browser_module_sha256, episodes['secant-turn'].runtime_build.browser_module_sha256);
}
const reference = structuredClone(recordings['reference-turn']);
delete reference.runtime.config.implicit.newton.broyden_updates;
const candidate = structuredClone(recordings['secant-turn']);
delete candidate.runtime.config.implicit.newton.broyden_updates;
assert.deepEqual(reference, candidate, 'Identical turning inputs and recipes except the secant flag');
const native = read('runs/full-robot/learning/whole-broyden/student-turn-20ms-broyden.native.json');
assert.deepEqual(native.recording, recordings['secant-turn'].runtime);
const parity = read(`${run}/parity.json`), ui = read(`${run}/leaderboard.json`);
assert(parity.passed && parity.replay_exact && parity.reset_exact && ui.passed);
const acceptance = read(`${run}/live-forward-acceptance/summary.json`);
assert.equal(acceptance.capture.sha256, source(`${run}/live-forward.native.json`).sha256);
const forward = read(`${run}/live-forward.native.json`);
assert.deepEqual(forward.recording, recordings['secant-forward'].runtime);
const buildPath = 'runs/wasm-builds/broyden-simd-lto/build.json';
const report = {version:1, build:read(buildPath), parity, ui, episodes, forward_acceptance:acceptance,
  all_realtime_gates_passed: Object.values(episodes).every(e => e.meets_speed_target && e.meets_transition_target),
  sources:[buildPath,`${run}/parity.json`,`${run}/leaderboard.json`,`${run}/live-forward.native.json`,`${run}/live-forward-acceptance/summary.json`,
    ...names.flatMap(n=>[`${run}/${n}.json`,`${run}/${n}.recording.json`]), `${root}/collect_broyden_browser.mjs`,
    'web/tests/live_performance.mjs','web/tests/leaderboard.mjs'].map(source),
  scope:'Sequential rendered keyboard cases without concurrent heavy work. Same WASM binary/compiler for the reference and secant steering pair; exact controls differ only by solver flag. Full native/WASM parity and UI tests pass. p95 remains above 20 ms. Native re-execution audits physical forward behavior separately from timing; no held-out robustness, sustained timestep or hardware validation.'};
writeFileSync(`${root}/broyden-browser-status.json`, JSON.stringify(report,null,2)+'\n');
console.log({forward_task_passed:acceptance.passed,all_realtime_gates_passed:report.all_realtime_gates_passed});
