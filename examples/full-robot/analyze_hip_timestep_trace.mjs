// Endpoint diagnostics; no alternate physics or controller implementation.
import {readFile, writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const root=process.argv[2]??'runs/full-robot/learning/hip-timestep';
const inputs={};
async function read(path){const b=await readFile(path);inputs[path]=createHash('sha256').update(b).digest('hex');return JSON.parse(b);}
const manifest=await read(root+'/manifest.json');
const markers=(await read('examples/full-robot/foot-markers.json')).markers;
const captures=await Promise.all(manifest.cases.map(c=>read(`${root}/${c.name}.window.json`)));
const exact=[];
function declaredEqual(expected,actual,path='config'){
 if(expected!==null&&typeof expected==='object'){
  assert(actual!==null&&typeof actual==='object',path);
  if(Array.isArray(expected))assert.equal(actual.length,expected.length,path);
  for(const [key,value] of Object.entries(expected))declaredEqual(value,actual[key],path+'.'+key);
 }else assert.equal(actual,expected,path);
}
for(let i=0;i<captures.length;i++){
 const d=captures[i]; assert(d.window_complete&&d.error===null);assert.equal(d.frames.length,951);
 // Rust adds serialized defaults; every explicitly authored field must match.
 declaredEqual(await read(manifest.cases[i].config),d.config);
 for(let k=0;k<d.frames.length;k++)assert(Math.abs(d.frames[k].time_s-(0.7+k*.001))<1e-10);
 const reference=manifest.cases[i].reference_capture;
 if(reference){const r=await read(reference);assert(r.completed&&r.error===null);let count=0;
  for(const f of r.frames){const g=d.frames.find(g=>Math.abs(g.time_s-f.time_s)<1e-10);if(g){assert(isDeepStrictEqual(f,g),`observer changed frame at ${f.time_s}`);count++;}}
  assert.equal(count,96);exact.push({case:manifest.cases[i].name,identical_full_frames:count});
 }
}
function point(f,m){const p=f.poses.find(p=>p.name===m.link);assert(p);return p.position_m.map((v,i)=>v+p.rotation[i].reduce((a,x,j)=>a+x*m.local_point_m[j],0));}
function hip(f,d){const mi=d.metadata.motor_components.findIndex(m=>m.dof==='joint.-Y | Hip servo output');assert(mi>=0);
 const [offset,count,joint]=d.metadata.motor_state_layout[mi];assert([3,4].includes(count));
 return {angle_rad:f.joint_positions[joint],velocity_rad_s:f.joint_velocities[joint],target_rad:f.servo_targets_rad[mi],reference_rad:f.reference_targets_rad?.[mi],
  current_a:f.motor_readings[mi].current_a,torque_nm:f.motor_readings[mi].shaft_torque_nm,voltage_v:f.driver_readings[mi].motor_voltage_v,
  gear_angle_rad:f.motor_states[offset+2],backlash_mode:count===4?f.motor_states[offset+3]:null,command:f.servo_commands[mi],
  policy_time_s:f.policy?.time_s,plus_y_support_n:f.policy?.observations['marker.+Y-foot-surface.floor_force_world.z']};}
const comparisons=[];
for(let i=0;i<captures.length-1;i++){
 const a=captures[i],b=captures[i+1];const traces=a.frames.map((f,k)=>{
  const g=b.frames[k];assert.equal(f.time_s,g.time_s);
  return {time_s:f.time_s,marker_errors_m:Object.fromEntries(markers.map(m=>{const p=point(f,m),q=point(g,m);return [m.id,Math.hypot(...p.map((v,j)=>v-q[j]))];})),hip:[hip(f,a),hip(g,b)]};
 });
 const stats=markers.map(m=>{let worst=traces.reduce((a,b)=>a.marker_errors_m[m.id]>=b.marker_errors_m[m.id]?a:b);return {id:m.id,maximum_difference_m:worst.marker_errors_m[m.id],rms_difference_m:Math.sqrt(traces.reduce((s,t)=>s+t.marker_errors_m[m.id]**2,0)/traces.length),worst_time_s:worst.time_s,hip_at_worst:worst.hip};});
 comparisons.push({cases:[manifest.cases[i].name,manifest.cases[i+1].name],markers:stats,hip_mode_disagreement_samples:traces.filter(t=>t.hip[0].backlash_mode!==t.hip[1].backlash_mode).length});
}
const series=captures.map((d,i)=>({case:manifest.cases[i].name,samples:d.frames.map(f=>({time_s:f.time_s,...hip(f,d),foot_world_m:point(f,markers[0])})),
 sampled_mode_changes:d.frames.flatMap((f,k)=>k&&hip(f,d).backlash_mode!==hip(d.frames[k-1],d).backlash_mode?[{interval_s:[d.frames[k-1].time_s,f.time_s],to:hip(f,d).backlash_mode}]:[])}));
const result={scope:'0.7–1.65 s window, sampled every 1 ms. Pairwise differences are not errors against hardware or a converged solution. Mode changes are bracketed between observations, not exact event times. Policy telemetry has a separate timestamp. Window success does not imply full-motion success.',exact_observer_checks:exact,comparisons,inputs};
await writeFile(root+'/analysis.json',JSON.stringify(result,null,2));
await writeFile(root+'/hip-series.json',JSON.stringify({scope:result.scope,series}));
console.log(JSON.stringify({exact,comparisons},null,2));
