import fs from'node:fs';import assert from'node:assert/strict';
const dir=import.meta.dirname,name=process.argv[2]||'warm/candidate',read=n=>JSON.parse(fs.readFileSync(dir+'/'+n+'.json'));
const baselineName=process.argv[3]||'baseline';
const baseline=read(baselineName+'.native'),candidate=read(name+'.native'),protocol=read('protocol');
const max={motor_angle_rad:0,link_position_m:0,current_a:0,contact_force_n:0};let contactIdentities=true,cadence=true;
assert(candidate.completed&&!candidate.error);assert.equal(candidate.frames.length,baseline.frames.length);
for(let k=0;k<baseline.frames.length;k++){
 const a=baseline.frames[k],b=candidate.frames[k];assert.equal(a.time_s,b.time_s);
 for(const i of baseline.metadata.joint_indices)max.motor_angle_rad=Math.max(max.motor_angle_rad,Math.abs(a.joint_positions[i]-b.joint_positions[i]));
 for(let i=0;i<a.poses.length;i++){assert.equal(a.poses[i].name,b.poses[i].name);max.link_position_m=Math.max(max.link_position_m,Math.hypot(...a.poses[i].position_m.map((v,j)=>v-b.poses[i].position_m[j])));}
 for(let i=0;i<a.motor_readings.length;i++)max.current_a=Math.max(max.current_a,Math.abs(a.motor_readings[i].current_a-b.motor_readings[i].current_a));
 for(let i=0;i<12;i++)for(const index of [3,4])cadence&&=a.servo_states[i*8+index]===b.servo_states[i*8+index];
 if(a.contacts.length!==b.contacts.length){contactIdentities=false;continue;}
 for(let i=0;i<a.contacts.length;i++){const x=a.contacts[i],y=b.contacts[i];contactIdentities&&=x.link===y.link&&x.other===y.other;max.contact_force_n=Math.max(max.contact_force_n,Math.hypot(...x.force_n.map((v,j)=>v-y.force_n[j])));}
}
const noFall=candidate.transitions.every(t=>t.speed&&t.speed.fallen===false);
const speedup=baseline.wall_s/candidate.wall_s,accepted=noFall&&Object.entries(max).every(([k,v])=>v<=protocol.gates[k])&&contactIdentities&&cadence&&speedup>=protocol.gates.candidate_minimum_speedup;
const stepTimes=[...candidate.transition_wall_s].sort((a,b)=>a-b),p95=stepTimes[Math.ceil(stepTimes.length*.95)-1];
const result={name,no_detected_fall:noFall,accepted_as_solver_optimization:accepted,realtime_accepted:accepted&&3/candidate.wall_s>=protocol.gates.realtime_sim_per_wall&&p95<=protocol.gates.policy_step_p95_s,policy_step_p95_s:p95,baseline_wall_s:baseline.wall_s,candidate_wall_s:candidate.wall_s,speedup,simulation_per_wall:3/candidate.wall_s,max_error:max,gates:protocol.gates,contactIdentities,cadence,scope:'Short fixed-input schedule only; sustained gait and other commands require separate qualification.'};fs.writeFileSync(dir+'/'+name+'.qualification.json',JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
