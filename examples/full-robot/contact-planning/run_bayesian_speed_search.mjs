// Sustained-speed experiments over an explicitly supplied horizon. Physics and
// Bayesian proposals remain in Rust; this driver only prepares and records runs.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {isDeepStrictEqual} from 'node:util';
import {measureSpeedRun} from './measure_speed_run.mjs';
const [specPath]=process.argv.slice(2);assert(specPath);
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const pin=path=>({path,sha256:hash(path)});
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v)+'\n',{flag:'wx'});
const spec=read(specPath),root=spec.output_directory;
const scene=read(spec.source_prefix+'.scene.json'),config=read(spec.source_prefix+'.config.json'),actions=read(spec.source_prefix+'.actions.json');
assert(scene.duration_s>0&&Math.abs(config.step_s*config.steps-scene.duration_s)<1e-8);
assert.equal(actions.length,Math.round(scene.duration_s/scene.period_s));
const index=name=>{const i=scene.controller.inputs.findIndex(c=>c.name===name);assert(i>=0);return i;};
const speedIndex=index('command.forward_speed'),gainIndex=index('command.tracking_gain'),packetIndex=index('command.packet_sequence');
assert(actions.every(row=>row.every((v,i)=>i===packetIndex||v===actions[0][i])),'continuous constant source commands required');
assert(isDeepStrictEqual(spec.problem.parameters.map(p=>p.name),['command_speed','tracking_gain','velocity_lead_factor']));
assert.equal(spec.problem.constraints.length,1);
fs.mkdirSync(root);
const binary=spec.runtime_examples+'/run_environment';
const context={version:1,inputs:[...['scene','config','actions'].map(k=>pin(spec.source_prefix+'.'+k+'.json')),pin(spec.task),...(spec.baseline_capture?[pin(spec.baseline_capture)]:[])],
  code:[import.meta.filename,'examples/full-robot/contact-planning/measure_speed_run.mjs'].map(pin),binaries:[binary,spec.selector].map(pin),
  duration_s:scene.duration_s,problem:spec.problem,seed:spec.seed,
  scope:'Maximize net horizontal displacement/full source duration, without falling. No gait-quality gates. Parameter windows are search domains to expand, not physical speed limits.'};
const contextId=crypto.createHash('sha256').update(JSON.stringify(context)).digest('hex'),problem={...spec.problem,context_id:contextId};
write(root+'/context.json',{...context,context_id:contextId});write(root+'/spec.json',spec);
const env={...process.env,OMP_NUM_THREADS:'1',VECLIB_MAXIMUM_THREADS:'1',RAYON_NUM_THREADS:'2',EGOBOX_LOG:'off'};
function execute(name,exe,args){
  const out=fs.openSync(root+'/'+name+'.stdout.log','wx'),err=fs.openSync(root+'/'+name+'.stderr.log','wx');
  let r;try{r=spawnSync(exe,args,{env,stdio:['ignore',out,err]});}finally{fs.closeSync(out);fs.closeSync(err);}
  write(root+'/'+name+'.execution.json',{binary:exe,args,exit_code:r.status,signal:r.signal,error:r.error?.message??null});
  return r;
}
let next=0;const observations=[];
function evaluate(values,method){
  assert.equal(values.length,3);assert(values.every((v,i)=>Number.isFinite(v)&&v>=problem.parameters[i].bounds[0]&&v<=problem.parameters[i].bounds[1]));
  const ordinal=next++,name='evaluation-'+String(ordinal).padStart(3,'0'),prefix=root+'/'+name,[speed,gain,lead]=values;
  const s=structuredClone(scene),a=structuredClone(actions);
  s.controller.inputs[speedIndex].lower=-Math.max(...problem.parameters[0].bounds.map(Math.abs));s.controller.inputs[speedIndex].upper=-s.controller.inputs[speedIndex].lower;
  Object.assign(s.controller.inputs[gainIndex],{lower:problem.parameters[1].bounds[0],upper:problem.parameters[1].bounds[1],initial:gain});
  s.controller.parameters.velocity_lead_s=scene.controller.parameters.velocity_lead_s.map(v=>v*lead);
  for(const row of a){row[speedIndex]=speed;row[gainIndex]=gain;}
  const restored=structuredClone(s);restored.controller.inputs=scene.controller.inputs;restored.controller.parameters.velocity_lead_s=scene.controller.parameters.velocity_lead_s;
  assert(isDeepStrictEqual(restored,scene),'unexpected scene mutation');
  write(prefix+'.scene.json',s);write(prefix+'.actions.json',a);fs.copyFileSync(spec.source_prefix+'.config.json',prefix+'.config.json',fs.constants.COPYFILE_EXCL);
  const args=[prefix+'.scene.json',prefix+'.config.json',spec.task,prefix+'.actions.json'],start=performance.now();
  const r=execute(name,binary,args);fs.renameSync(prefix+'.stdout.log',prefix+'.native.json');
  let capture=null,metrics=null,outcome,baselineReplay=null;
  try{capture=read(prefix+'.native.json');}catch{}
  if(capture?.error===null&&!r.error){
    metrics=measureSpeedRun(capture);write(prefix+'.summary.json',metrics);
    outcome=(r.status===0||metrics.fallen)&&(metrics.completed||metrics.fallen)
      ?{status:'complete',objective:-metrics.speed_m_s,residuals:[metrics.fallen?1:-1]}
      :{status:'failed',reason:'incomplete or abnormal runtime exit'};
    if(ordinal===0&&spec.baseline_capture){
      const reference=read(spec.baseline_capture);assert(capture.completed&&reference.completed);assert.equal(capture.frames.length,reference.frames.length);
      const keys=['time_s','poses','joint_positions','joint_velocities','servo_targets_rad','motor_states','motor_readings','contacts','contact_history'];
      for(let i=0;i<capture.frames.length;i++)for(const k of keys)assert(isDeepStrictEqual(capture.frames[i][k],reference.frames[i][k]),`baseline physical replay differs at frame ${i}, ${k}`);
      baselineReplay={frames:capture.frames.length,identical_physical_fields:keys};
    }
  }else outcome={status:'failed',reason:capture?.error??r.error?.message??'runtime produced no valid capture'};
  const observation={context_id:contextId,values,outcome,evidence:prefix+'.evaluation.json'};
  write(observation.evidence,{observation,method,metrics,baseline_replay:baselineReplay,wall_s:(performance.now()-start)/1000,capture_sha256:hash(prefix+'.native.json')});
  observations.push(observation);write(prefix+'.observations.json',observations);
  console.log(JSON.stringify({name,values,outcome,baseline_replay:baselineReplay}));
}
evaluate(spec.baseline_values,'baseline');
for(const values of spec.seed_values??[])evaluate(values,'declared-seed');
if(spec.initial_design_count){
  const request=root+'/initial.request.json',output=root+'/initial.design.json';write(request,{problem,count:spec.initial_design_count,seed:spec.seed});
  assert.equal(execute('initial-design',spec.selector,['--design',request,output]).status,0);
  for(const values of read(output).values)evaluate(values,'latin-hypercube');
}
for(let i=0;i<spec.adaptive_evaluations;i++){
  const request=root+`/adaptive-${i}.request.json`,output=root+`/adaptive-${i}.proposal.json`;
  write(request,{problem,observations,config:{seed:spec.seed+i,acquisition_starts:8,maximum_training_rows:256}});
  assert.equal(execute('adaptive-'+i,spec.selector,[request,output]).status,0);
  evaluate(read(output).proposal.values,'constrained-log-expected-improvement');
}
const best=observations.filter(o=>o.outcome.status==='complete'&&o.outcome.residuals.every(v=>v<=0)).sort((a,b)=>a.outcome.objective-b.outcome.objective)[0]??null;
write(root+'/result.json',{problem,duration_s:scene.duration_s,observations,best,scope:context.scope});
