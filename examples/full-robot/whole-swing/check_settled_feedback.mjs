import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', combined = process.argv.includes('--combined'), plan = JSON.parse(readFileSync(`${root}/${combined ? 'combined' : 'settled'}-plan.json`));
const original = JSON.parse(readFileSync(`${root}/sustained-plan.json`)), cases = [];
for (const c of plan.cases) {
  const capturePath = c.config.replace('.config.json', '.native.json');
  const capture = JSON.parse(readFileSync(capturePath)); assert(capture.completed && !capture.error);
  const oldCase = original.cases.find(old => old.step_s === c.step_s), oldPath = !combined && oldCase?.config.replace('.config.json', '.native.json');
  const old = oldPath ? JSON.parse(readFileSync(oldPath)) : null, parameters = capture.recording.scene.controller.parameters;
  let boosted = 0, firstBoost = null, preserved = 0;
  for (let i = 1; i < capture.frames.length; i++) {
    const frame = capture.frames[i], p = frame.policy, sensors = p.observations;
    const stopped = parameters.motion_command_channels.every(name => sensors[name] === 0);
    const support = Math.min(1, ...parameters.support_force_channels.map(name => Math.max(0, Math.min(1, sensors[name] / parameters.full_support_force_n))));
    const errors = [0, 0];
    for (const [name, target] of Object.entries(p.targets)) {
      const joint = name.slice(0, -7), reference = sensors[`${joint}.reference`];
      for (let mode = 0; mode < 2; mode++) {
        const increment = parameters.standing_gain_increment + (mode ? parameters.settled_gain_increment : 0);
        const bodyGain = sensors['command.body_gain'] + (stopped ? increment * support : 0);
        const expected = reference + sensors['command.tracking_gain'] * (reference - sensors[`${joint}.angle`])
          + bodyGain * sensors[`${joint}.body_correction`] + sensors['command.point_gain'] * sensors[`${joint}.point_correction`]
          + sensors[parameters.residual_input_by_target[name]];
        errors[mode] = Math.max(errors[mode], Math.abs(target - expected));
      }
    }
    assert(Math.min(...errors) <= 1e-10, `unexpected teacher authority at ${frame.time_s}`);
    if (errors[0] > 1e-10) {
      assert(stopped); assert.equal(p.step_reference.reference.phase, 'idle');
      boosted++; firstBoost ??= frame.time_s;
    }
    if (firstBoost === null && old) {
      for (const key of ['poses', 'joint_positions', 'joint_velocities', 'servo_targets_rad', 'contacts']) assert.deepEqual(frame[key], old.frames[i][key], `${c.name} unchanged transfer ${frame.time_s} ${key}`);
      preserved++;
    }
  }
  assert(boosted >= 10); assert(firstBoost > (c.duration_s === 60 ? 56 : 20));
  cases.push({name: c.name, boosted_idle_frames: boosted, first_boost_frame_s: firstBoost, unchanged_pre_boost_frames: preserved,
    sources: [capturePath, oldPath].filter(Boolean).map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')}))});
}
writeFileSync(`${root}/${combined ? 'combined' : 'settled'}-feedback-check.json`, JSON.stringify({passed: true, cases,
  scope: 'Actual recorded motor targets use only the original or declared settled gain. Every observed added-gain frame is idle; where an original-teacher capture is included, all physical frames before the first boost are identical.'}, null, 2) + '\n');
console.log(cases.map(c => ({name: c.name, first_boost_frame_s: c.first_boost_frame_s, boosted_idle_frames: c.boosted_idle_frames})));
