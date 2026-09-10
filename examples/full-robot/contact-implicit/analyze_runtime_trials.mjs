// Recorded measurements only. Shared Rust supplies references, physics and geometry.
import fs from 'node:fs';import assert from 'node:assert/strict';import crypto from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import {recordedContactMotion} from '../../interactive/recorded_contact_motion.mjs';
const root='examples/full-robot/contact-implicit/',runtime='runs/contact-implicit/';
const read=p=>JSON.parse(fs.readFileSync(p));const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
// Avoid dumping CAD payloads on a mismatch. Captures serialize typed schemas,
// adding defaults and dropping export-only fields, so raw scene equality is invalid.
const equal=(a,b,label)=>assert(isDeepStrictEqual(a,b),label);
const reference=read(process.argv[2]??root+'surface25-translating.compiled.json');
const names=process.argv.length>3?process.argv.slice(3):['surface-tracking-ff64-v3','surface-tracking-ff128-v3','surface-tracking-pd64-v3'];
assert(names.length>0 && names.every(name=>/^[a-z0-9-]+$/.test(name)));
const captures=names.map(name=>read(runtime+name+'.native.json'));
const body=frame=>frame.poses.find(p=>p.name==='Robot | Chassis and hip mounts');
const summaries=captures.map((capture,index)=>{
  const name=names[index],trial=read(root+name+'.trial.json'),geometry=read(runtime+name+'.geometry.json');
  const supplied=read(runtime+name+'.scene.json');
  equal(capture.recording.scene.robot.source,supplied.robot.source,'CAD source provenance changed');
  // JSON key order is not semantic; compare the explicitly supplied options.
  for(const [key,value] of Object.entries(supplied.options))assert.deepEqual(capture.recording.scene.options[key],value,`option ${key}`);
  assert.equal(sha(runtime+name+'.scene.json'),trial.files.find(f=>f.path.endsWith('.scene.json')).sha256);
  assert.equal(geometry.capture_completed,capture.completed);
  assert.equal(capture.frames.length,geometry.frames.length);
  assert(capture.completed && capture.error===null);
  const frames=capture.frames,first=body(frames[0]),last=body(frames.at(-1)),duration=frames.at(-1).time_s;
  assert(Math.abs(duration-reference.duration_s)<1e-12);
  const initial=reference.planning.frames[0].position.slice(0,3).map((v,j)=>v-reference.step_s*reference.planning.frames[0].velocity[j]);
  assert(Math.hypot(...first.position_m.map((v,j)=>v-initial[j]))<1e-12,'initial body pose must match');
  const jointErrors=[],bodyErrors=[];
  for(let k=0;k<reference.trajectory.keyframes.length;k++){
    const knot=reference.trajectory.keyframes[k],frame=frames.reduce((a,b)=>Math.abs(b.time_s-knot.time_s)<Math.abs(a.time_s-knot.time_s)?b:a);
    assert(Math.abs(frame.time_s-knot.time_s)<1e-12);
    const expected=k===0?initial:reference.planning.frames[k-1].position.slice(0,3);
    bodyErrors.push(Math.hypot(...body(frame).position_m.map((v,j)=>v-expected[j])));
    for(let j=0;j<knot.values.length;j++)jointErrors.push(Math.abs(frame.joint_positions[capture.metadata.joint_indices[j]]-knot.values[j]));
  }
  let bodyPath=0;for(let i=1;i<frames.length;i++)bodyPath+=Math.hypot(...body(frames[i]).position_m.slice(0,2).map((v,j)=>v-body(frames[i-1]).position_m[j]));
  const feet=capture.recording.config.policy.task_observations.markers.map(marker=>{
    const linkIndex=capture.recording.scene.robot.links.findIndex(l=>l.name===marker.link);
    const samples=frames.map(frame=>recordedContactMotion(frame,linkIndex,marker.link));let distance=0;
    for(let i=1;i<samples.length;i++){const a=samples[i-1],b=samples[i];if(a.force>=1 && b.force>=1)distance+=(b.time_s-a.time_s)*(a.pointSpeed+b.pointSpeed)/2;}
    return{link:marker.link,loaded_material_path_m:distance,ratio_to_body_path:distance/bodyPath};
  });
  return{name,completed:capture.completed,step_s:capture.recording.config.step_s,frames:frames.length,
    duration_s:duration,feedforward_gain:trial.feedforward_gain,
    measured_mean_diagonal_body_speed_m_s:(last.position_m[0]-first.position_m[0]+last.position_m[1]-first.position_m[1])/Math.sqrt(2)/duration,
    measured_terminal_diagonal_body_speed_m_s:(last.velocity_m_s[0]+last.velocity_m_s[1])/Math.sqrt(2),
    maximum_body_position_error_at_plan_knots_m:Math.max(...bodyErrors),
    maximum_joint_error_at_plan_knots_rad:Math.max(...jointErrors),
    rms_joint_error_at_plan_knots_rad:Math.sqrt(jointErrors.reduce((n,v)=>n+v*v,0)/jointErrors.length),
    maximum_tilt_rad:Math.max(...frames.map(f=>Math.acos(Math.max(-1,Math.min(1,body(f).rotation[2][2]))))),
    maximum_sampled_inter_link_penetration_m:Math.max(...geometry.frames.map(f=>f.maximum_inter_link_penetration_m)),
    minimum_sampled_floor_clearance_m:Math.min(...geometry.frames.flatMap(f=>f.floor_clearances.map(p=>p.minimum_clearance_m))),
    maximum_loaded_material_slip_ratio:Math.max(...feet.map(f=>f.ratio_to_body_path)),feet,body_path_m:bodyPath,
    capture_sha256:sha(runtime+name+'.native.json'),wall_s:capture.wall_s};
});
let timestep=null;
if(captures.length>=2) {
equal(captures[0].recording.scene.robot,captures[1].recording.scene.robot,'timestep robot mismatch');
equal(captures[0].recording.scene.controller,captures[1].recording.scene.controller,'timestep controller mismatch');
equal(captures[0].recording.config.motors,captures[1].recording.config.motors,'timestep motors mismatch');
equal(captures[0].recording.config.policy,captures[1].recording.config.policy,'timestep policy mismatch');
const options0={...captures[0].recording.scene.options},options1={...captures[1].recording.scene.options};delete options0.step;delete options1.step;assert.deepEqual(options0,options1);
timestep={maximum_body_path_difference_m:Math.max(...captures[0].frames.map((frame,i)=>{
  assert(Math.abs(frame.time_s-captures[1].frames[i].time_s)<1e-12);
  return Math.hypot(...body(frame).position_m.map((v,j)=>v-body(captures[1].frames[i]).position_m[j]));})),
  relative_mean_speed_difference:Math.abs(summaries[0].measured_mean_diagonal_body_speed_m_s/summaries[1].measured_mean_diagonal_body_speed_m_s-1)};
}
const firstPlan=reference.planning.frames[0],lastPlan=reference.planning.frames.at(-1);
const initialPlan=firstPlan.position.slice(0,2).map((v,i)=>v-reference.step_s*firstPlan.velocity[i]);
const plannedMean=(lastPlan.position[0]-initialPlan[0]+lastPlan.position[1]-initialPlan[1])/Math.sqrt(2)/reference.duration_s;
console.log(JSON.stringify({scope:`Short start-from-rest tracking diagnostic. Errors at ${reference.trajectory.keyframes.length} planning knots; contact motion and geometry at ${captures[0].frames.length} report poses. No continuous collision, sustained gait or WASD qualification.`,
  planned_mean_speed_m_s:plannedMean,summaries,timestep},null,2));
