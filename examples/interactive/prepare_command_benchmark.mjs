// Build a scheduled keyboard experiment using the viewer's actual command map.
// All physical execution stays in the shared Rust environment.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {driveMotionValues,motionCommandConfig,motionHeartbeatIndex,nextMotionAction} from '../../web/viewer/motion-commands.mjs';
const [source,presetPath,protocolPath,output]=process.argv.slice(2);
assert(output,'usage: prepare_command_benchmark source-recording preset protocol fresh-input');
const read=p=>JSON.parse(fs.readFileSync(p));
const r=read(source),preset=read(presetPath),protocol=read(protocolPath);
assert.equal(protocol.version,1);assert(Array.isArray(protocol.stages)&&protocol.stages.length>0);
const channels=r.runtime.scene.controller.inputs,drive=motionCommandConfig(preset,channels);
assert(drive,'preset has no typed motion command mapping');
const indices=drive.command_channels.map(name=>channels.findIndex(c=>c.name===name));
const dt=r.runtime.config.step_s,period=r.task.period_s;
const ticks=(seconds,step)=>{const n=seconds/step;assert(Number.isFinite(n)&&n>0&&Number.isSafeInteger(Math.round(n))&&Math.abs(n-Math.round(n))<1e-8,'duration must lie on the physics/task grid');return Math.round(n);};
let action=r.runtime.input_events[0]?.at_step===0?[...r.runtime.input_events[0].values]:channels.map(c=>c.initial);
const heartbeat=motionHeartbeatIndex(preset,channels);
if(heartbeat>=0)action[heartbeat]=channels[heartbeat].initial;
const stride=ticks(period,dt);let intervals=0;
const events=[],stages=[];
for(const stage of protocol.stages){
 assert(Array.isArray(stage.keys)&&new Set(stage.keys).size===stage.keys.length&&stage.keys.every(k=>'wasd'.includes(k)&&k.length===1));
 const count=ticks(stage.duration_s,period),motion=driveMotionValues(preset,channels,new Set(stage.keys));
 const start=intervals*period;
 for(let i=0;i<count;i++){
  indices.forEach((index,j)=>{action[index]=motion[j];});
  action=nextMotionAction(preset,channels,action);
  events.push({at_step:intervals*stride,values:[...action]});intervals++;
 }
 stages.push({...stage,start_s:start,end_s:intervals*period,requested_motion:motion});
}
r.runtime.config.steps=intervals*stride;
r.runtime.completed_steps=r.runtime.config.steps;
r.runtime.scene.duration_s=intervals*period;
r.runtime.input_events=events;
// Read-only task observations for projected heading, including tilted bodies.
// These add no controller input, reward, termination condition or physics change.
const body=r.task.speed?.body_link??r.task.progress?.link;
assert(body,'a progress reference body is required for heading measurements');
for(const axis of ['x','y','z']){
 const name=`benchmark.forward.${axis}`;
 assert(!r.task.observations.some(o=>o.name===name));
 r.task.observations.push({name,source:{kind:'body_axis',link:body,body_axis:'x',world_axis:axis}});
}
const bytes=JSON.stringify(r)+'\n';
fs.writeFileSync(output,bytes,{flag:'wx'});
fs.writeFileSync(output+'.spec.json',JSON.stringify({version:1,kind:'planned_command_benchmark',source,preset:presetPath,protocol:protocolPath,
 input_sha256:createHash('sha256').update(bytes).digest('hex'),duration_s:intervals*period,stages,
 changes:['runtime.config.steps','runtime.completed_steps','runtime.scene.duration_s','runtime.input_events','task.observations: read-only projected body axis'],
 scope:'Planned input in replay-compatible format; completed_steps declares scheduled input coverage, not a measured completion. Same robot, controller, fixed gains, timestep and seed. Keyboard vectors use the production viewer mapping.'},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({output,duration_s:intervals*period,actions:events.length,stages:stages.length}));
