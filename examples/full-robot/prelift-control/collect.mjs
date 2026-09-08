// Audit exact recipes, canceled transfers and independent physical acceptance.
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual as equal} from 'node:util';
import assert from 'node:assert/strict';
const root='examples/full-robot/prelift-control';
const run=process.argv[2]??'runs/full-robot/learning/prelift-control';
const read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const plan=read(`${run}/plan.json`);
for(const s of plan.sources)assert.equal(source(s.path).sha256,s.sha256);
const markersPath='examples/full-robot/foot-markers.json',markers=read(markersPath);
// Rust supplies explicit optional defaults on serialization. Every authored
// field must still match; extra default fields do not change the source recipe.
function authored(a,b,path='recipe',allowOmittedMetadata=false){
 if(a&&typeof a==='object'){
  assert(b&&typeof b==='object',path);
  if(Array.isArray(a))assert.equal(a.length,b.length,path);
  for(const k of Object.keys(a)){
   if(allowOmittedMetadata&&!Object.hasOwn(b,k))continue;
   authored(a[k],b[k],`${path}.${k}`,allowOmittedMetadata);
  }
 }else assert(equal(a,b),path);
}
const position=(f,m)=>{const p=f.poses.find(p=>p.name===m.link);assert(p);return p.position_m.map((v,i)=>v+p.rotation[i].reduce((s,r,j)=>s+r*m.local_point_m[j],0));};
const distance=(a,b)=>Math.hypot(...a.map((v,i)=>v-b[i]));
function cancellations(c){
 const out=[],frames=c.frames;
 for(let i=0;i<frames.length;i++){
  const ref=frames[i].policy?.step_reference?.reference;
  if(ref?.phase!=='recenter')continue;
  const start=i,previous=frames[i-1].policy.step_reference.reference;
  assert.equal(previous.phase,'shift');
  const initialWalking=c.transitions[i-1].walking;
  let maximumMarkerDisplacement=0,minimumForce=Infinity;
  while(i<frames.length&&frames[i].policy.step_reference.reference.phase==='recenter'){
   const f=frames[i],r=f.policy.step_reference.reference,w=c.transitions[i].walking;
   assert.equal(r.foot,null);assert.equal(r.step,previous.step);
   assert(equal(r.latched_twist,[0,0,0]));assert(equal(r.feet_world_m,previous.feet_world_m));
   assert.equal(w.qualified_steps,initialWalking.qualified_steps);assert.equal(w.failed_steps,initialWalking.failed_steps);
   assert.equal(w.step_reward,0);assert.equal(w.outcome,null);
   for(const m of markers.markers)maximumMarkerDisplacement=Math.max(maximumMarkerDisplacement,distance(position(f,m),position(frames[start],m)));
   minimumForce=Math.min(minimumForce,...f.policy.step_reference.measured_support_force_n);
   i++;
  }
  assert(i<frames.length,'canceled transfer never finishes recentering');
  const last=frames[i],r=last.policy.step_reference.reference;
  assert(['idle','shift'].includes(r.phase));assert.equal(r.step,previous.step);
  const threshold=c.recording.config.policy.step_reference.minimum_support_force_n;
  const count=Math.round(c.recording.config.policy.step_reference.sequence.qualification_s/c.task.period_s);
  for(let j=i-count+1;j<=i;j++)assert(frames[j].policy.step_reference.measured_support_force_n.every(f=>f>=threshold),'all-foot qualification before leaving recenter');
  out.push({start_s:frames[start].time_s,end_s:last.time_s,step:previous.step,next_phase:r.phase,
   planted_references_unchanged:true,walking_credit_unchanged:true,exit_support_qualified:true,
   maximum_actual_marker_displacement_m:maximumMarkerDisplacement,minimum_sampled_support_force_n:minimumForce});
 }
 return out;
}
const captures=new Map(),cases=[];
let parsedScene;
for(const recipe of plan.cases){
 const path=`${run}/${recipe.name}.native.json`,c=read(path),acceptancePath=`${run}/${recipe.name}-acceptance/summary.json`,a=read(acceptancePath);
 assert(c.completed&&!c.error);assert(a.passed);assert.equal(a.capture.sha256,source(path).sha256);
 // CAD carries additional provenance/display fields not in the Rust scene
 // schema. Compare all retained fields to CAD, and the entire parsed scene
 // across runs so no physical field can vary between experiments unnoticed.
 authored(read(plan.scene),c.recording.scene,'scene',true);
 if(parsedScene)assert(equal(parsedScene,c.recording.scene));else parsedScene=c.recording.scene;
 authored(read(recipe.config),c.recording.config);assert(equal(read(plan.task),c.task));
 assert.equal(c.recording.config.policy.step_reference.sequence.update_command_before_lift??false,recipe.enabled!==false);
 assert.equal(markers.expected_cad_sha256,c.recording.scene.robot.source.cad_sha256);
 const actions=read(recipe.actions),stride=Math.round(c.task.period_s/c.recording.config.step_s);
 let held=c.recording.scene.controller.inputs.map(i=>i.initial),eventIndex=0;
 for(let i=0;i<actions.length;i++){
  if(c.recording.input_events[eventIndex]?.at_step===i*stride)held=c.recording.input_events[eventIndex++].values;
  assert(equal(held,actions[i]),`${recipe.name} action ${i}`);
 }
 assert.equal(eventIndex,c.recording.input_events.length);assert.equal(c.frames.length,actions.length+1);
 const canceled=cancellations(c);
 if(recipe.enabled!==false&&recipe.name.startsWith('cancel'))assert.equal(canceled.length,1);
 captures.set(recipe.name,c);
 cases.push({name:recipe.name,passed:a.passed,cancellations:canceled,lifts:a.lifts.length,
  final_body_error_m:a.final_body_error_m,final_yaw_error_rad:a.final_yaw_error_rad,
  maximum_body_tilt_rad:a.maximum_body_tilt_rad,sampled_internal_contacts:a.sampled_internal_contacts,
  sources:[source(path),source(acceptancePath),source(recipe.config),source(recipe.actions)]});
}
const strip=f=>{const g=structuredClone(f);delete g.stepping_wall_s;return g;};
const preserved=[['default-turn','enabled-turn'],['default-early-reverse','prelift-reverse']].map(([a,b])=>{
 const x=captures.get(a),y=captures.get(b);assert(equal(x.frames.map(strip),y.frames.map(strip)));assert(equal(x.transitions,y.transitions));
 return {baseline:a,candidate:b,frames:x.frames.length,physical_frames_and_task_transitions_exact:true};
});
const refinements=['cancel-resume','prelift-turn'].map(name=>{
 const path=`${run}/${name}-refinement.json`,r=read(path);
 assert.equal(r.baseline.sha256,source(`${run}/${name}.native.json`).sha256);
 assert.equal(r.candidate.sha256,source(`${run}/${name}-refined.native.json`).sha256);
 return {name,...r,source:source(path)};
});
const prototype=read(plan.prototype.capture);assert(!prototype.completed&&prototype.error);
const result={version:1,cases,preserved,refinements,rejected_prototype:{...plan.prototype,error:prototype.error,source:source(plan.prototype.capture)},
 sources:[source(`${run}/plan.json`),source(markersPath),source(`${root}/collect.mjs`)],
 scope:'Sampled 24-second flat-floor cases with identical CAD physics, weights and motor limits. Canceled transfers preserve foot references and earn no walking credit; actual marker motion is measured separately. Independent executed-step, endpoint and geometry checks pass. Timestep differences are reported, not hidden by those task checks. No general command, physical stopping-time, sustained performance, terrain or hardware-transfer certificate.'};
writeFileSync(`${root}/status.json`,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({cases:cases.map(c=>({name:c.name,lifts:c.lifts,cancellations:c.cancellations})),preserved,refinements:refinements.map(r=>({name:r.name,foot_difference_m:r.metrics.foot_marker_position_m.maximum,phase_mismatches:r.phase_mismatches}))},null,2));
