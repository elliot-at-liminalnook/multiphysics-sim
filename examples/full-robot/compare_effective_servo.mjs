// Compare physical marker trajectories; no physics is reimplemented here.
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [referencePath,candidatePath,output]=process.argv.slice(2);
assert(referencePath&&candidatePath&&output,'reference.json candidate.json report.json');
const read=p=>JSON.parse(readFileSync(p)),hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const a=read(referencePath),b=read(candidatePath),markers=read('examples/full-robot/foot-markers.json');
assert(a.completed&&b.completed);assert.equal(a.frames.length,b.frames.length);
for(const r of [a,b])assert.equal(r.recording.scene.robot.source.cad_sha256,markers.expected_cad_sha256);
const point=(f,m)=>{const p=f.poses.find(p=>p.name===m.link);assert(p);return p.position_m.map((x,i)=>x+p.rotation[i].reduce((s,r,j)=>s+r*m.local_point_m[j],0));};
const foot=Object.fromEntries(markers.markers.map(m=>[m.id,0]));let body=0,angle=0,penetration=0,torque=0;
for(let i=0;i<a.frames.length;i++){
 const x=a.frames[i],y=b.frames[i];assert(Math.abs(x.time_s-y.time_s)<1e-9);
 for(const m of markers.markers){const p=point(x,m),q=point(y,m);foot[m.id]=Math.max(foot[m.id],Math.hypot(...p.map((v,j)=>v-q[j])));}
 const p=x.poses.find(p=>p.name==='Robot | Chassis and hip mounts'),q=y.poses.find(p=>p.name==='Robot | Chassis and hip mounts');
 assert(p&&q);body=Math.max(body,Math.hypot(...p.position_m.map((v,j)=>v-q.position_m[j])));
 for(const [j,o] of b.contract.observations.entries())if(o.name.endsWith('.angle')){
   const k=a.contract.observations.findIndex(p=>p.name===o.name);assert(k>=0);
   angle=Math.max(angle,Math.abs(a.transitions[i].observations[k]-b.transitions[i].observations[j]));
 }
 for(const c of y.contacts)penetration=Math.max(penetration,c.penetration_m);
 for(const m of y.motor_readings)torque=Math.max(torque,Math.abs(m.shaft_torque_nm));
}
const times=[...(b.transition_wall_s||[])].sort((a,b)=>a-b);
const report={version:1,reference:{path:referencePath,sha256:hash(referencePath)},candidate:{path:candidatePath,sha256:hash(candidatePath)},
 completed:true,simulated_s:b.transitions.at(-1).time_s,wall_s:b.wall_s,
 simulation_per_wall_second:b.transitions.at(-1).time_s/b.wall_s,
 transition_p95_s:times.length?times[Math.ceil(times.length*.95)-1]:null,
 maximum_foot_difference_m:foot,maximum_body_position_difference_m:body,
 maximum_motor_angle_difference_rad:angle,maximum_penetration_m:penetration,maximum_effective_torque_nm:torque,
 scope:'Single native capture comparison over one short stepping reference. Endpoint samples at 50 Hz; does not bound between-sample peaks, validate impact impulses, establish sustained gait or hardware accuracy.'};
writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));
