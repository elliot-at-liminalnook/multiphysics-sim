// Compare terminal executions of the shared Rust evaluator; no physics here.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as equal} from 'node:util';
const [directory,mode] = process.argv.slice(2);
assert(directory && (!mode || mode==='--after-only'), 'usage: check_identification_resolution case-directory [--after-only]');
const read = name => JSON.parse(fs.readFileSync(path.join(directory,name+'.json')));
const result = {};
const difference = (a,b) => Math.max(...a.frames.flatMap((frame,i)=>frame.joint_positions.map((q,j)=>Math.abs(q-b.frames[i].joint_positions[j]))));
for (const robot of ['wheeled','quadruped']) {
  function cases(phase) {
    const cases = Object.fromEntries(['original','identified','explicit'].map(name=>[name,read(`${robot}-${phase}-${name}-native`)]));
    for (const c of Object.values(cases)) {
      assert(!c.experiment.spec.config.motors?.effective, 'identification acceptance must exercise detailed motor equations');
      assert(c.passed && c.all_replayed_and_resumed_frames_exact);
      assert(c.full.final_transition.truncated && !c.full.final_transition.terminated);
      assert.equal(c.full.recording.error,null);
      assert(equal(c.experiment.runtime,cases.original.experiment.runtime));
    }
    return cases;
  }
  const after = cases('after');
  assert(equal(after.identified.frames,after.explicit.frames), 'identified and explicitly resolved physical trajectories differ');
  assert(equal(after.identified.full.final_transition,after.explicit.full.final_transition), 'identified and explicit outcomes differ');
  const change = difference(after.original,after.identified);
  assert(change>0, 'identified parameters must affect physical motion');
  const original = after.original.experiment.spec.scene.robot;
  const identified = after.identified.full.recording.runtime.scene.robot;
  assert(equal(identified.motors,original.motors), 'recording must retain original unfitted motors');
  assert(Object.keys(identified.identification).length>0, 'fit must survive recording');
  const explicit = after.explicit.full.recording.runtime.scene.robot;
  assert.equal(Object.keys(explicit.identification).length,0);
  assert(equal(explicit.source.resolved_identification.identification,identified.identification));
  let beforeDifference = null;
  if (!mode) {
    const before = cases('before');
    beforeDifference = difference(before.identified,before.explicit);
    assert(beforeDifference>0, 'historical mismatch was not reproduced');
    assert(equal(before.original.frames,after.original.frames), 'unidentified baseline behavior changed');
    assert(equal(before.explicit.frames,after.explicit.frames), 'explicitly resolved baseline behavior changed');
    assert(!equal(before.original.experiment.runtime,after.original.experiment.runtime));
  }
  result[robot] = {identified_and_explicit_frames_and_outcomes_exact:true,all_replays_exact:true,
    maximum_identification_joint_change_rad:change,before_fix_identified_vs_explicit_maximum_joint_difference_rad:beforeDifference,
    unchanged_original_and_explicit_baselines_checked:!mode,runtime:after.identified.experiment.runtime};
}
const report={version:1,passed:true,robots:result,
  scope:'Synthetic identification resolution and replay acceptance on two CAD forms. Not a hardware fit, sustained locomotion, convergence or realtime qualification.'};
fs.writeFileSync(path.join(directory,'identification-acceptance.json'),JSON.stringify(report,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify(report,null,2));
