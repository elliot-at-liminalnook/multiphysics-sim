// Read-only diagnostics of the existing CAD-derived controller captures.
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/dynamics-breakdown';
const inputs={};
async function read(p){const b=await readFile(p);inputs[p]=createHash('sha256').update(b).digest('hex');return JSON.parse(b);}
const comparison=await read(root+'/timestep.comparison.json');
const runs=await Promise.all(['base','refined'].map(n=>read(`runs/full-robot/learning/analytic-positions/${n}.execution.json`)));
assert(runs.every(r=>r.completed&&r.error===null));
const worst=comparison.markers.reduce((a,b)=>a.maximum_error_m>b.maximum_error_m?a:b);
const frames=runs.map(r=>{const f=r.frames.find(f=>Math.abs(f.time_s-worst.worst_time_s)<1e-10);assert(f);return f;});
assert.equal(frames[0].reference_time_s,frames[1].reference_time_s);
const bodyName='Robot | Chassis and hip mounts';
const bodies=frames.map(f=>f.poses.find(p=>p.name===bodyName));assert(bodies.every(Boolean));
const motors=['-Y | Hip servo output','-Y | Worm servo output','-Y | Foot servo output'].map(name=>{
 const key=name+'.angle',angles=frames.map(f=>f.policy.observations[key]);assert(angles.every(Number.isFinite));
 return {name,angles_rad:angles,difference_rad:angles[0]-angles[1],difference_deg:(angles[0]-angles[1])*180/Math.PI};
});
const windows=[[0,0.3],[0.3,0.75],[0.75,1.2],[1.2,1.65],[1.65,2.1],[2.1,2.8]].map(([start,end])=>{
 const samples=comparison.sample_differences.filter(s=>s.time_s>=start-1e-10&&s.time_s<=end+1e-10);
 assert(samples.length);return {start_s:start,end_s:end,maximum_marker_error_m:Math.max(...samples.flatMap(s=>Object.values(s.marker_errors_m)))};
});
const result={scope:'Current local-motor-solve controller, 0.25 versus 0.125 ms, same model/recipe. Sampled disagreement is not error against an established converged or hardware reference. Motor observations are sampled at the policy timestamp; poses are at the reporting timestamp. This identifies a diagnostic target, not a proven causal explanation.',
 worst_marker:worst,nominal_time_windows:windows,report_time_s:frames[0].time_s,reference_time_s:frames[0].reference_time_s,
 policy_times_s:frames.map(f=>f.policy.time_s),motors,body_poses:bodies,
 force_plus_y_n:frames.map(f=>f.policy.observations['marker.+Y-foot-surface.floor_force_world.z']),
 contact_pairs:comparison.sample_differences.find(s=>Math.abs(s.time_s-worst.worst_time_s)<1e-10),
 next_test:'Trace loaded hip angle, target, current, shaft torque, backlash mode and contact state around 1.2–1.5 s across finer timesteps; separate integration damping/phase effects from physical actuator response and feedback changes before choosing a larger-step or reduced-actuator model.',inputs};
await writeFile(root+'/timestep-diagnosis.json',JSON.stringify(result,null,2));
console.log(JSON.stringify({worst_marker:worst,motors,windows},null,2));
