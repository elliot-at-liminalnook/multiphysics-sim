// Verify completed Rust cases; no motion generation or physical stepping here.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const [directory] = process.argv.slice(2);
assert(directory, 'usage: check_coordinated_motion completed-case-directory');
const read = name => JSON.parse(fs.readFileSync(path.join(directory,name+'.json')));
const robots = {};
for (const robot of ['quadruped','wheeled']) {
  const original = read(robot+'-original-native');
  const identity = read(robot+'-identity-native');
  const changed = read(robot+'-changed-native');
  for (const c of [original,identity,changed]) {
    assert(c.passed && c.all_replayed_and_resumed_frames_exact);
    assert(c.full.final_transition.truncated && !c.full.final_transition.terminated);
    assert.equal(c.full.recording.error, null);
  }
  assert.deepEqual(identity.frames, original.frames, 'identity changed physical trajectory');
  assert.deepEqual(identity.full.final_transition, original.full.final_transition);
  assert.deepEqual(identity.full.recording.runtime.scene, original.full.recording.runtime.scene);
  assert.equal(original.frames.length, changed.frames.length);
  let maximumJointChange = 0;
  for (let i=0; i<original.frames.length; i++) {
    assert.equal(original.frames[i].time_s, changed.frames[i].time_s);
    original.frames[i].joint_positions.forEach((q,j)=>{
      maximumJointChange = Math.max(maximumJointChange, Math.abs(q-changed.frames[i].joint_positions[j]));
    });
  }
  assert(maximumJointChange>1e-12, 'changed recipe must affect physical joint motion');
  for (const field of ['config','task','seed','source_actions']) assert.deepEqual(changed.experiment.spec[field],original.experiment.spec[field]);
  assert.deepEqual(changed.experiment.spec.scene,original.experiment.spec.scene,'source scene mutated');
  const before = original.full.recording.runtime, after = changed.full.recording.runtime;
  for (const field of ['config','seed','completed_steps']) assert.deepEqual(after[field],before[field]);
  for (const field of ['robot','options','period_s','duration_s']) assert.deepEqual(after.scene[field],before.scene[field]);
  robots[robot] = {identity_frames_and_task_exact:true, all_variants_replay_exact:true,
    maximum_joint_position_change_rad:maximumJointChange,frames:original.frames.length,
    runtime:changed.experiment.runtime,context_id:changed.experiment.context_id};
}
const report = {passed:true,robots,scope:'Short physical controller sensitivity and exact identity/replay. Not a sustained speed, physical convergence or realtime acceptance.'};
fs.writeFileSync(path.join(directory,'native-acceptance.json'),JSON.stringify(report,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify(report,null,2));
