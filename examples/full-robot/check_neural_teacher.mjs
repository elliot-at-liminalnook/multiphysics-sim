// Artifact and trajectory checks independent of reward-based policy selection.
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [baselinePath,zeroPath,learnedPath,output]=process.argv.slice(2);
assert(output,'usage: baseline zero-neural learned capture-report.json');
const read=p=>JSON.parse(readFileSync(p));
const digest=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const baseline=read(baselinePath),zero=read(zeroPath),learned=read(learnedPath);
for(const c of [baseline,zero,learned])assert(c.completed&&!c.error);
assert.equal(baseline.frames.length,zero.frames.length);
for(let i=0;i<baseline.frames.length;i++)for(const [key,value] of Object.entries(baseline.frames[i])){
 if(key==='stepping_wall_s')continue;
 if(key==='policy'){for(const [k,v] of Object.entries(value??{}))assert.deepEqual(zero.frames[i].policy[k],v);}
 else assert.deepEqual(zero.frames[i][key],value,`zero neural changed physical frame ${i}.${key}`);
}
const artifact=read('examples/full-robot/neural-teacher/policy.json');
assert.deepEqual(learned.recording.config.policy.neural_residual,artifact);
let maximum=0,nonzero=0;
for(let i=0;i<learned.frames.length;i++){
 const frame=learned.frames[i],t=learned.transitions[i];
 for(const o of artifact.outputs){
  const index=learned.contract.observations.findIndex(c=>c.source.kind==='neural_correction'&&c.source.actuator===o.target);
  assert(index>=0);assert.equal(learned.contract.observations[index].unit,'rad');
  const actual=frame.policy?.neural_residual?.[o.target]??0;
  assert.equal(t.observations[index],actual);assert(Math.abs(actual)<=o.scale);
  maximum=Math.max(maximum,Math.abs(actual));if(actual!==0)nonzero++;
 }
}
assert(nonzero>0,'learned artifact must produce actual motor corrections');
writeFileSync(output,JSON.stringify({version:1,passed:true,baseline:digest(baselinePath),zero:digest(zeroPath),learned:digest(learnedPath),policy:digest('examples/full-robot/neural-teacher/policy.json'),
 exact_zero_physical_frames:zero.frames.length,maximum_correction_rad:maximum,nonzero_corrections:nonzero,
 scope:'Zero neural output preserves the baseline physical trajectory. The selected artifact generates bounded corrections whose held values reach teacher observations. Walking acceptance, held-out rewards, timestep sensitivity and browser parity are separate gates.'},null,2)+'\n');
console.log(output);
