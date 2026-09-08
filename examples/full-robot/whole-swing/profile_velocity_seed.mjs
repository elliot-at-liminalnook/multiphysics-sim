import {readFileSync,writeFileSync,existsSync,openSync,closeSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',run='runs/full-robot/learning/whole-velocity-seed',read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const clean=x=>JSON.parse(JSON.stringify(x,(k,v)=>k==='stepping_wall_s'?undefined:v));
const plan=read(`${run}/plan.json`),status=read(`${root}/velocity-seed-status.json`);assert(status.complete);
const cases=[];
for(const c of plan.cases){
  const capture=`${run}/${c.name}.profiled.native.json`,profile=`${run}/${c.name}.profile.json`,log=`${run}/${c.name}.profile.log`;
  assert(!existsSync(capture)&&!existsSync(profile)&&!existsSync(log));
  const out=openSync(capture,'wx'),err=openSync(log,'wx');
  const result=spawnSync('target/release/examples/run_environment',[c.scene,c.config,c.task,c.actions,'--profile',profile],{stdio:['ignore',out,err]});
  closeSync(out);closeSync(err);assert(!result.error&&result.status===0);
  const measured=read(capture),original=read(`${run}/${c.name}.native.json`),p=read(profile);assert(measured.completed&&p.completed);
  assert.deepEqual(clean(measured.frames),clean(original.frames),'profiling must preserve every physical/task frame');
  assert.deepEqual(measured.recording,original.recording);
  cases.push({name:c.name,profile_preserves_frames:true,wall_s:p.wall_s,buckets:p.buckets,accepted_steps:p.accepted_implicit_steps.length,
    predicted_steps:p.accepted_implicit_steps.filter(d=>d.predicted_velocity_seed).length,
    accepted_endpoint_evaluations:p.accepted_implicit_steps.reduce((n,d)=>n+d.endpoint_evaluations,0),
    exact_derivative_fallbacks:p.accepted_implicit_steps.flatMap(d=>d.exact_jacobian_fallback?[d.exact_jacobian_fallback]:[]),
    sources:[capture,profile,log,`${run}/${c.name}.native.json`].map(source)});
}
const oldPath='runs/full-robot/learning/whole-tangent-radius/tangent-radius-0.00001.native.json';
const previous=read(oldPath),reference=read(`${run}/velocity-seed-reference.native.json`);
assert.deepEqual(clean(reference.frames),clean(previous.frames),'default-off must preserve all previously accepted frames');
assert.deepEqual(reference.recording,previous.recording);assert.deepEqual(reference.task,previous.task);
const report={version:1,complete:true,default_off_frames_exact:true,cases,
  sources:[`${run}/plan.json`,`${root}/velocity-seed-status.json`,oldPath,'target/release/examples/run_environment',import.meta.filename].map(source),
  scope:'Isolated sequential profiles; profiling preserves all accepted physical/task frames and recordings. Global solver buckets include rejected attempts but the Newton residual bucket excludes the separate initial-guess screen; endpoint/closure buckets include that work. Accepted-step counts omit rejected outer attempts. Timings nest. Native throughput is not rendered performance or physical accuracy.'};
writeFileSync(`${root}/velocity-seed-profile.json`,JSON.stringify(report,null,2)+'\n');console.log(cases.map(c=>({name:c.name,wall_s:c.wall_s,predicted_steps:c.predicted_steps})));
