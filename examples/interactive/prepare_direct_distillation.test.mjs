import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const program=fileURLToPath(new URL('./prepare_direct_distillation.mjs',import.meta.url));
function fixture(t){
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'direct-imitation-'));
  t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  const write=(name,value)=>{const p=path.join(root,name);fs.writeFileSync(p,JSON.stringify(value));return p;};
  const pin=p=>({path:p,sha256:crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex')});
  const actuator={name:'joint.target',kind:'Angle',unit:'rad'};
  function capture(angle,target){return {completed:true,error:null,recording:{scene:{duration_s:.02}},
    metadata:{policy_contract:{actuators:[actuator],observations:[{name:'joint.angle',kind:'Angle',unit:'rad'}],software_target_bounds_rad:[[-1,1]]}},
    frames:[{time_s:.02,servo_targets_rad:[target],policy:{time_s:0,observations:{'joint.angle':angle},targets:{'joint.target':target}}}]};}
  const learner=pin(write('learner.json',capture(.2,.1))),validation=pin(write('validation.json',capture(.3,.2)));
  const labels={source_capture:learner.path,actuators:[actuator],samples:[{policy_time_s:0,targets_rad:[.7]}]};
  const policy={version:1,features:[{source:'joint.angle',subtract:null,kind:'Angle',center:12,scale:3,clip:1e6}],
    outputs:[{target:'joint.target',kind:'Angle',scale:1}],layers:[{weights:[[.2]],biases:[.1]}]};
  const descriptor={version:1,output:path.join(root,'output'),seed:42,hidden_widths:[],features:['joint.angle'],
    training:[{capture:learner}],validation:[{capture:validation}],optimizer:{epochs:1,learning_rate:.001,batch_size:1},initial_policy:pin(write('initial.json',policy))};
  const run=mutate=>{
    mutate?.(labels,descriptor);
    descriptor.training[0].labels={...pin(write('labels.json',labels)),capture_sha256:learner.sha256};
    const result=spawnSync(process.execPath,[program,write('spec.json',descriptor)],{encoding:'utf8'});
    return {result,descriptor,learner,pin};
  };
  return {run,policy};
}
test('counterfactual advice trains on learner observations while preserving measured actions and warm-start normalization',t=>{
  const {run,policy}=fixture(t),{result,descriptor,learner,pin}=run();
  assert.equal(result.status,0,result.stderr);
  const e=JSON.parse(fs.readFileSync(path.join(descriptor.output,'experiment.json')));
  assert.deepEqual(e.training.samples,[{observations:[.2],actions:[.7]}]);
  assert.deepEqual(e.validation.samples,[{observations:[.3],actions:[.2]}]);
  assert.deepEqual(e.network,policy);
  assert.equal(pin(learner.path).sha256,learner.sha256);
});
test('teacher advice from another decision time is rejected before creating output',t=>{
  const {result,descriptor}=fixture(t).run(labels=>{labels.samples[0].policy_time_s=.02;});
  assert.notEqual(result.status,0);assert.match(result.stderr,/teacher label time mismatch/);
  assert(!fs.existsSync(descriptor.output));
});
test('counterfactual advice cannot silently exceed the actuator range',t=>{
  const {result,descriptor}=fixture(t).run(labels=>{labels.samples[0].targets_rad[0]=1.2;});
  assert.notEqual(result.status,0);assert(!fs.existsSync(descriptor.output));
});
