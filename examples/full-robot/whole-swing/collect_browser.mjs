import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', run = 'runs/interactive/whole-swing';
const read = p => JSON.parse(readFileSync(p)), source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const forward = read(`${run}/live-forward.json`), turn = read(`${run}/live-turn-reverse-recheck.json`);
const acceptance = read(`${run}/live-forward-acceptance/summary.json`), failed = read(`${run}/failed-steering.native.json`);
const failedRecord = read(`${run}/live-turn-reverse-recheck.recording.json`);
assert(forward.completed && forward.keyboard_commands_recorded); assert(!turn.completed && turn.simulation_error);
assert.equal(acceptance.capture.sha256, source(`${run}/live-forward.native.json`).sha256);
assert(!failed.completed && failed.error); assert.equal(failed.recording.completed_steps, failedRecord.runtime.completed_steps);
assert.equal(failed.error.split(':')[0], turn.simulation_error.split(':')[0]);
assert.deepEqual(failed.recording.input_events, failedRecord.runtime.input_events);
const report = {version: 1, forward, forward_acceptance: acceptance, turn,
  failed_steering_reproduced_natively: {error: failed.error, completed_steps: failed.recording.completed_steps,
    input_events: failed.recording.input_events, seed: failed.recording.seed},
  parity: read(`${root}/browser-parity.json`),
  sources: [`${run}/live-forward.json`, `${run}/live-forward.recording.json`, `${run}/live-forward.native.json`,
    `${run}/live-turn-reverse-recheck.json`, `${run}/live-turn-reverse-recheck.recording.json`, `${run}/failed-steering.actions.json`, `${run}/failed-steering.native.json`,
    `${root}/browser.scene.json`, `${root}/browser.config.json`, 'examples/full-robot/heading-task/task.json',
    'web/tests/live_performance.mjs', `${root}/collect_browser.mjs`].map(source),
  scope: 'Development-only 3.75 mm/s live keyboard experiments. Forward physical audit passes; active rate and processing p95 fail. Turning loses planned static support before reverse is reached. No complete steering, sustained-speed, robustness or sim-to-real acceptance.'};
writeFileSync(`${root}/browser-status.json`, JSON.stringify(report, null, 2) + '\n');
console.log(report.scope);
