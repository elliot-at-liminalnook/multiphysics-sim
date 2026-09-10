// Longer-distance and timestep evidence through the same Rust environment.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {measureSpeedRun} from './measure_speed_run.mjs';
const [source,root,order='fine-first']=process.argv.slice(2);assert(source&&root);
assert(['fine-first','long-first'].includes(order));
fs.mkdirSync(root);
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v,null,2)+'\n',{flag:'wx'});
const binary='/Users/elliot/physics-simulator/target/gait-exploration/release/examples/run_environment';
const task='examples/full-robot/contact-planning/speed-discovery-task.json';
const baseScene=read(source+'.scene.json'),baseConfig=read(source+'.config.json'),baseActions=read(source+'.actions.json');
const baseline=read(source+'.native.json'),baselineMetric=measureSpeedRun(baseline);
assert(baselineMetric.completed&&!baselineMetric.fallen);
const packet=baseScene.controller.inputs.findIndex(ch=>ch.name==='command.packet_sequence');assert(packet>=0);
for(const row of baseActions)assert(row.every((v,i)=>i===packet||v===baseActions[0][i]),'continuous constant motion required');
write(root+'/launch.json',{source,order,binary,binary_sha256:hash(binary),task,task_sha256:hash(task),
  inputs:['scene','config','actions','native'].map(s=>({path:source+'.'+s+'.json',sha256:hash(source+'.'+s+'.json')})),
  code:[import.meta.filename,'examples/full-robot/contact-planning/measure_speed_run.mjs'].map(path=>({path,sha256:hash(path)})),baseline:baselineMetric});
const rows=[];
const cases=[['fine20',20,.0003125],['long60',60,.000625]];
if(order==='long-first')cases.reverse();
for(const [name,duration,step] of cases){
  const prefix=root+'/'+name,scene=structuredClone(baseScene),config=structuredClone(baseConfig);
  scene.duration_s=duration;config.step_s=step;config.steps=Math.round(duration/step);config.report_every=Math.round(scene.period_s/step);
  const actions=Array.from({length:Math.round(duration/scene.period_s)},(_,i)=>baseActions[0].map((v,j)=>j===packet?i+1:v));
  for(const [suffix,value] of [['scene',scene],['config',config],['actions',actions]])write(prefix+'.'+suffix+'.json',value);
  const args=[prefix+'.scene.json',prefix+'.config.json',task,prefix+'.actions.json'];
  const out=fs.openSync(prefix+'.native.json','wx'),err=fs.openSync(prefix+'.log','wx'),start=performance.now();
  let result;try{result=spawnSync(binary,args,{stdio:['ignore',out,err],env:{...process.env,OMP_NUM_THREADS:'1',VECLIB_MAXIMUM_THREADS:'1'}});}
  finally{fs.closeSync(out);fs.closeSync(err);}
  const execution={binary,args,exit_code:result.status,signal:result.signal,error:result.error?.message??null,wall_s:(performance.now()-start)/1000};
  write(prefix+'.execution.json',execution);
  let capture,metrics=null,error=execution.error;
  try{capture=read(prefix+'.native.json');if(capture.error===null)metrics=measureSpeedRun(capture);else error=capture.error;}catch(e){error=e.message;}
  let comparison=null;
  if(metrics?.completed){
    const body=f=>f.poses.find(p=>p.name==='Robot | Chassis and hip mounts');
    let maximumBodyDifference=0;assert(capture.frames.length>=baseline.frames.length);
    for(let i=0;i<baseline.frames.length;i++){
      const a=baseline.frames[i],b=capture.frames[i];assert(Math.abs(a.time_s-b.time_s)<1e-8);
      maximumBodyDifference=Math.max(maximumBodyDifference,Math.hypot(...body(a).position_m.map((v,j)=>v-body(b).position_m[j])));
    }
    comparison={first_20s_maximum_body_difference_m:maximumBodyDifference,
      full_duration_speed_difference_fraction:metrics.speed_m_s/baselineMetric.speed_m_s-1};
  }
  const row={name,prefix,duration_s:duration,step_s:step,execution,metrics,error,comparison,capture_sha256:hash(prefix+'.native.json')};
  write(prefix+'.summary.json',row);rows.push(row);console.log(JSON.stringify(row));
}
write(root+'/summary.json',{baseline:baselineMetric,rows,scope:'Continuous distance/time and sampled fall detection only. Finer integration and longer travel are independent physical evidence, with discrepancies reported directly; no development gait-quality gates.'});
