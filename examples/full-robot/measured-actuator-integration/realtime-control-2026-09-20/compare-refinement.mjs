import fs from 'node:fs';
import assert from 'node:assert/strict';
const root=import.meta.dirname;
const read=p=>JSON.parse(fs.readFileSync(`${root}/${p}.json`));
const gates=read('protocol').gates;
assert(!process.argv[2] || process.argv[3], "candidate requires reference");
const pairs=process.argv[2] ? [[process.argv[2],process.argv[3]]] : [
 ['warm-probes/shared-body','reference-12800hz/reference'],
 ['coupled-sdirk2/step-1/reference','reference-12800hz/reference'],
 ['coupled-sdirk2/step-4/candidate','coupled-sdirk2/step-1/reference'],
 ['coupled-sdirk2/step-8/candidate','coupled-sdirk2/step-1/reference'],
];
const results=pairs.map(([candidate,reference])=>{
 const a=read(reference+'.native'),b=read(candidate+'.native');
 assert(a.completed&&!a.error&&b.completed&&!b.error);
 assert.equal(a.frames.length,b.frames.length);
 const max={motor_angle_rad:0,link_position_m:0,current_a:0,contact_force_n:0};
 let contactIdentities=true,cadence=true;
 for(let j=0;j<a.frames.length;j++){
  const x=a.frames[j],y=b.frames[j];assert.equal(x.time_s,y.time_s);
  for(const i of a.metadata.joint_indices)max.motor_angle_rad=Math.max(max.motor_angle_rad,Math.abs(x.joint_positions[i]-y.joint_positions[i]));
  assert.equal(x.poses.length,y.poses.length);
  for(let i=0;i<x.poses.length;i++){
   assert.equal(x.poses[i].name,y.poses[i].name);
   max.link_position_m=Math.max(max.link_position_m,Math.hypot(...x.poses[i].position_m.map((v,k)=>v-y.poses[i].position_m[k])));
  }
  assert.equal(x.motor_readings.length,y.motor_readings.length);
  for(let i=0;i<x.motor_readings.length;i++)max.current_a=Math.max(max.current_a,Math.abs(x.motor_readings[i].current_a-y.motor_readings[i].current_a));
  for(let i=0;i<12;i++)for(const index of [3,4])cadence&&=x.servo_states[i*8+index]===y.servo_states[i*8+index];
  if(x.contacts.length!==y.contacts.length){contactIdentities=false;continue;}
  for(let i=0;i<x.contacts.length;i++){
   contactIdentities&&=x.contacts[i].link===y.contacts[i].link&&x.contacts[i].other===y.contacts[i].other;
   max.contact_force_n=Math.max(max.contact_force_n,Math.hypot(...x.contacts[i].force_n.map((v,k)=>v-y.contacts[i].force_n[k])));
  }
 }
 assert(Object.values(max).every(Number.isFinite));
 return {candidate,reference,max_error:max,contactIdentities,cadence,
  passes_existing_physical_gates:contactIdentities&&cadence&&Object.entries(max).every(([k,v])=>v<=gates[k]),
  candidate_wall_s:b.wall_s,reference_wall_s:a.wall_s};
});
const report={version:1,gates,results,scope:'Three-second fixed-input numerical sensitivity. The referenced recipes and protocols specify timestep and solver-tolerance changes; model, seed and motor/controller clocks must match. Neither reference is assumed exact. These comparisons do not establish convergence or hardware accuracy. Performance gates are separate.'};
const output=process.argv[4] || (process.argv[2] ? process.argv[2]+'.refinement' : 'refinement-comparison');
fs.writeFileSync(`${root}/${output}.json`,JSON.stringify(report,null,2));
console.log(JSON.stringify(report,null,2));
