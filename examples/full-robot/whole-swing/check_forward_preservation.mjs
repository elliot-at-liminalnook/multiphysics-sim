import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing';
const paths = ['runs/full-robot/learning/whole-finish/finish-0.85-20ms.native.json', 'runs/full-robot/learning/whole-steering/student-forward-20ms.native.json'];
const [a, b] = paths.map(p => JSON.parse(readFileSync(p)));
assert(a.completed && b.completed && !a.error && !b.error);
assert.deepEqual(a.recording.input_events, b.recording.input_events); assert.equal(a.recording.seed, b.recording.seed);
assert.deepEqual(a.recording.scene.robot, b.recording.scene.robot); assert.deepEqual(a.recording.scene.options, b.recording.scene.options);
assert.deepEqual(a.task, b.task); assert.equal(a.frames.length, b.frames.length);
const metrics = {};
function compare(a, b, key) {
  if (typeof a === 'number') { assert(Number.isFinite(a) && Number.isFinite(b)); metrics[key] = Math.max(metrics[key] ?? 0, Math.abs(a - b)); }
  else if (a && typeof a === 'object') { assert.deepEqual(Object.keys(a), Object.keys(b)); for (const k of Object.keys(a)) compare(a[k], b[k], key); }
  else assert.equal(a, b);
}
for (let i = 0; i < a.frames.length; i++) {
  for (const key of ['joint_positions', 'joint_velocities', 'poses', 'servo_targets_rad', 'reference_targets_rad', 'contacts']) compare(a.frames[i][key], b.frames[i][key], key);
  compare(a.frames[i].policy?.step_reference?.reference, b.frames[i].policy?.step_reference?.reference, 'planned_reference');
}
const passed = Object.values(metrics).every(v => v <= 1e-9);
writeFileSync(`${root}/forward-preservation.json`, JSON.stringify({passed, frames: a.frames.length, maximum_absolute_differences: metrics,
  sources: [...paths, `${root}/check_forward_preservation.mjs`].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Full-forward physical and planned frames with the explicit positive-speed posture knot. 1e-9 numeric preservation check, not physical accuracy.'}, null, 2) + '\n');
assert(passed); console.log(metrics);
