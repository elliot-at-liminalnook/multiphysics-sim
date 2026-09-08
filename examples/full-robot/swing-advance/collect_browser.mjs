import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const run = process.argv[2] ?? 'runs/interactive/swing-advance';
const root = 'examples/full-robot/swing-advance';
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const parity = read(`${run}/overlap-browser.json`), live = read(`${run}/live-forward.json`);
const viewer = read(`${run}/viewer-all-report.json`), association = read(`${run}/live-forward-association.json`);
const acceptance = read(`${run}/live-forward-acceptance/summary.json`);
assert(parity.passed && parity.replay_exact && parity.reset_exact && parity.difference_count === 0);
assert(live.completed && live.validation_passed && live.errors.length === 0);
assert(viewer.passed && association.passed && acceptance.passed);
for (const id of ['robot-fast-student', 'robot-swing-advance', 'robot-prelift-control']) {
  assert(viewer.checks.some(c => c.startsWith(id + ':') && c.includes('visible replay')));
}
assert.equal(parity.config_override.sha256, source(`${root}/half.config.json`).sha256);
assert.equal(acceptance.capture.sha256, source(`${run}/live-forward.native.json`).sha256);
for (const s of association.sources) assert.equal(source(s.path).sha256, s.sha256);
const recipe = read(`${run}/live-forward.recording.json`);
assert.equal(recipe.runtime.config.policy.step_reference.sequence.swing_body_advance_fraction, .5);
assert.equal(recipe.runtime.config.policy.step_reference.sequence.maximum_speed_m_s, .0025);
const {numeric_groups, ...compactParity} = parity;
const report = {version: 1, parity: compactParity, live, viewer, association,
  physical_reexecution: {passed: acceptance.passed, lifts: acceptance.lifts.length,
    failed_lifts: acceptance.lifts.filter(l => !l.passed).length,
    final_body_error_m: acceptance.final_body_error_m, final_yaw_error_rad: acceptance.final_yaw_error_rad,
    maximum_body_tilt_rad: acceptance.maximum_body_tilt_rad, sampled_internal_contacts: acceptance.sampled_internal_contacts,
    budgets: acceptance.budgets},
  sources: [ `${run}/overlap-browser.json`, `${run}/live-forward.json`, `${run}/viewer-all-report.json`,
    `${run}/live-forward-association.json`, `${run}/live-forward.recording.json`, `${run}/live-forward.native.json`,
    `${run}/live-forward-acceptance/summary.json`, `${run}/viewer/build-manifest.json`,
    `${root}/half.config.json`, 'examples/full-robot/student-speed/scene.json',
    'web/tests/viewer.mjs', 'web/tests/environment.mjs', 'web/tests/live_performance.mjs',
    'crates/sim-domain-control/src/stepping.rs', 'crates/sim-runtime/examples/run_environment.rs',
    `${root}/collect_browser.mjs`].map(source),
  scope: 'A runnable experimental half-overlap milestone. Exact loaded configuration identity, native/WASM parity and replay are checked independently of sampled physical acceptance and rendered timing. Active 1x and 20 ms p95 requirements remain failed; no promotion, sustained-minute, broader steering, terrain or hardware-transfer claim.'};
writeFileSync(`${root}/browser-status.json`, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({parity: parity.passed, viewer_checks: viewer.checks.length,
  physical_reexecution: acceptance.passed, active: live.performance.active_motion,
  speed_target: live.meets_speed_target, transition_target: live.meets_transition_target}));
