import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const run = 'runs/interactive/wasm-profiles', read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const scalar = read('runs/wasm-builds/scalar-release/build.json'), simd = read('runs/wasm-builds/simd-lto/build.json');
assert(scalar.completed && simd.completed); assert.deepEqual(scalar.sources, simd.sources);
assert.equal(scalar.rustc, simd.rustc); assert.equal(scalar.cargo, simd.cargo);
const parity = {scalar: read(`${run}/scalar-parity.json`), simd: read(`${run}/simd-parity.json`)};
assert(Object.values(parity).every(p => p.passed && p.replay_exact && p.reset_exact));
const ui = read(`${run}/simd-leaderboard.json`); assert(ui.passed);
const episodes = [], inputs = [];
for (const scenario of ['forward', 'turn']) {
  const captures = {};
  for (const profile of ['scalar', 'simd']) {
    const name = `${profile}-${scenario}`, path = `${run}/${name}.json`, recordingPath = `${run}/${name}.recording.json`;
    const report = read(path), recording = read(recordingPath);
    assert(report.completed && report.validation_passed && report.keyboard_commands_recorded);
    assert.equal(recording.error, null); assert.equal(recording.runtime.seed, 0);
    captures[profile] = recording;
    episodes.push({name, ...report}); inputs.push(path, recordingPath);
  }
  assert.deepEqual(captures.scalar, captures.simd, `${scenario}: compiler comparison must replay identical inputs and recipes`);
}
const report = {version: 1, builds: {scalar, simd}, parity, ui, episodes,
  all_realtime_gates_passed: episodes.every(e => e.meets_speed_target && e.meets_transition_target),
  sources: [...inputs, 'web/leaderboard/SIMD-PLAN.md', 'web/leaderboard/collect_wasm_profiles.mjs',
    'web/build-wasm.mjs', 'web/build-viewer.mjs', 'web/tests/live_performance.mjs'].map(source),
  scope: 'Sequential rendered keyboard benchmarks on the recorded host, without concurrent builds/simulations. Same Rust source, controller, world, seed and complete input recordings across compiler profiles. SIMD/LTO improves processing modestly; neither profile satisfies active rate and p95 requirements. No physics, accuracy budget or motor authority changes.'};
writeFileSync('web/leaderboard/wasm-profile-status.json', JSON.stringify(report, null, 2) + '\n');
console.log(episodes.map(e => ({name: e.name, active_rate: e.performance.active_motion.simulation_per_wall_second, active_p95_ms: e.performance.active_motion.transition_p95_s * 1000})));
