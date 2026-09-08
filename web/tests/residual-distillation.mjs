import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,readFileSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
function fixture(change=()=>{}){
  const dir=mkdtempSync(join(tmpdir(),'residual-data-'));
  const put=(name,value)=>{const path=join(dir,`${name}.json`),bytes=JSON.stringify(value);writeFileSync(path,bytes);return {path,sha256:createHash('sha256').update(bytes).digest('hex')};};
  const scene={robot:{source:{cad_sha256:'fixture'}},options:{},controller:{inputs:[]}},config={step_s:.01,steps:2,policy:{}},task={version:1};
  const observations=[{name:'q',kind:'Angle'},{name:'r',kind:'Angle'},{name:'gain',kind:'Dimensionless'}];
  const make=event=>({completed:true,error:null,recording:{scene,config,completed_steps:2,input_events:[event]},task,
    metadata:{policy_contract:{observations,actuators:[{name:'motor.target',kind:'Angle'}]}},
    frames:[{time_s:.02,servo_targets_rad:[.24],policy:{time_s:0,observations:{q:.1,r:.2,gain:.25},targets:{'motor.target':.24}}}]});
  const a=make({at_step:0,values:[1]}),b=make({at_step:0,values:[2]});change(a,b);
  const train=put('train',a),validation=put('validation',b);
  const descriptor={version:1,output:join(dir,'output'),scene:put('scene',scene),config:put('config',config),task:put('task',task),scene_schema_omissions:put('omissions',{fields:[]}),
    network:put('network',{version:1,features:[{source:'q',kind:'Angle',scale:1,center:0,subtract:null,clip:5}],outputs:[{target:'motor.target',kind:'Angle',scale:.05}],layers:[]}),
    training:[{capture:train,acceptance:put('acceptance',{passed:true,capture:train})}],validation:[{capture:validation,allow_accepted_prefix:true}],
    baseline:[{target:'motor.target',reference:'r',position:'q',gain:'gain'}],optimizer:{epochs:1,learning_rate:.001,batch_size:1}};
  const path=put('descriptor',descriptor).path;
  return {dir,run:()=>spawnSync(process.execPath,['examples/interactive/prepare_residual_distillation.mjs',path],{encoding:'utf8'}),read:name=>JSON.parse(readFileSync(join(dir,'output',`${name}.json`)))};
}
test('teacher labels use the recorded gain and preserve failed validation prefixes',()=>{
  const f=fixture((a,b)=>{b.completed=false;b.error='reserved episode stopped';});
  try{const result=f.run();assert.equal(result.status,0,result.stderr);const e=f.read('experiment');assert(Math.abs(e.training.samples[0].actions[0]-.015)<1e-15);assert.equal(f.read('manifest').inventory[1].completed,false);assert.equal(e.validation.samples.length,1);}finally{rmSync(f.dir,{recursive:true,force:true});}
});
test('a label inconsistent with the applied target is rejected',()=>{
  const f=fixture(a=>{a.frames[0].servo_targets_rad[0]=.23;});
  try{const r=f.run();assert.notEqual(r.status,0);assert.match(r.stderr,/applied servo command/);}finally{rmSync(f.dir,{recursive:true,force:true});}
});
test('duplicate policy samples and reused validation inputs are rejected',()=>{
  for(const change of [a=>a.frames.push(structuredClone(a.frames[0])),(a,b)=>b.recording.input_events=a.recording.input_events]){
    const f=fixture(change);try{assert.notEqual(f.run().status,0);}finally{rmSync(f.dir,{recursive:true,force:true});}
  }
});
