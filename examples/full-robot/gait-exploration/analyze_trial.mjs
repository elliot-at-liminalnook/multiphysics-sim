// Analysis of recorded Rust state only; no simulation or alternate contact law.
import fs from 'node:fs';
import crypto from 'node:crypto';
import {recordedContactMotion} from '../../interactive/recorded_contact_motion.mjs';
const dir='examples/full-robot/gait-exploration',name=process.argv[2],definition=process.argv[3]??name;
const def=JSON.parse(fs.readFileSync(`${dir}/candidates.json`)).definitions.find(d=>d.name===definition);
const file=`${dir}/${name}.native.json`,bytes=fs.readFileSync(file),c=JSON.parse(bytes),frames=c.frames;
const body=f=>f.poses.find(p=>p.name==='Robot | Chassis and hip mounts');
const near=t=>frames.reduce((a,f)=>Math.abs(f.time_s-t)<Math.abs(a.time_s-t)?f:a);
const stop=Number((.4+def.cycles*def.groups.length*def.transfer_s).toFixed(8));
const start=.4+def.groups.length*def.transfer_s;
const selected=frames.filter(f=>f.time_s>=start-1e-9&&f.time_s<=stop+1e-9);
const mean=a=>a.reduce((s,x)=>s+x,0)/a.length;
let mt=mean(selected.map(f=>f.time_s)),mx=mean(selected.map(f=>body(f).position_m[0]));
const speed=selected.reduce((s,f)=>s+(f.time_s-mt)*(body(f).position_m[0]-mx),0)/selected.reduce((s,f)=>s+(f.time_s-mt)**2,0);
const delta=body(frames.at(-1)).position_m.map((x,i)=>x-body(frames[0]).position_m[i]);
const net=Math.hypot(...delta.slice(0,2));
const motor=c.metadata.coordinate_names.map((name,i)=>({name,maximum_speed_rad_s:Math.max(...frames.map(f=>Math.abs(f.motor_readings[i].gear_speed_rad_s))),maximum_torque_nm:Math.max(...frames.map(f=>Math.abs(f.motor_readings[i].shaft_torque_nm))),angle_span_rad:Math.max(...frames.map(f=>f.joint_positions[c.metadata.joint_indices[i]]))-Math.min(...frames.map(f=>f.joint_positions[c.metadata.joint_indices[i]]))}));
let work=0;for(let i=1;i<frames.length;i++)for(let j=0;j<motor.length;j++){let p=f=>Math.max(0,f.motor_readings[j].gear_speed_rad_s*f.motor_readings[j].shaft_torque_nm);work+=(frames[i].time_s-frames[i-1].time_s)*(p(frames[i])+p(frames[i-1]))/2}
const feet=c.recording.config.policy.point_feedback.markers.map(m=>{
 let index=c.recording.scene.robot.links.findIndex(l=>l.name===m.link),samples=frames.map(f=>recordedContactMotion(f,index,m.link)),path=0;
 for(let i=1;i<samples.length;i++){let a=samples[i-1],b=samples[i];if(a.force>=1&&b.force>=1)path+=(b.time_s-a.time_s)*(a.speed+b.speed)/2}
 return {link:m.link,loaded_material_motion_m:path,motion_to_body_advance_ratio:path/net};
});
const timings=[...c.transition_wall_s].sort((a,b)=>a-b);
const out={version:1,completed:c.completed,error:c.error,scope:'Short offline-reference development trial, not sustained/steering/learning qualification. Recorded force-free inter-link contacts require a separate independent geometric audit.',capture_sha256:crypto.createHash('sha256').update(bytes).digest('hex'),definition,simulated_s:frames.at(-1).time_s,speed_window_s:[start,Math.min(stop,frames.at(-1).time_s)],measured_forward_speed_mm_s:frames.at(-1).time_s>=stop?speed*1000:null,accepted_prefix_forward_speed_mm_s:speed*1000,body_displacement_world_m:delta,stop_drift_m:frames.at(-1).time_s<stop?null:Math.hypot(...body(frames.at(-1)).position_m.slice(0,2).map((x,i)=>x-body(near(stop)).position_m[i])),maximum_heading_error_rad:Math.max(...frames.map(f=>Math.abs(Math.atan2(body(f).rotation[1][0],body(f).rotation[0][0])))),maximum_tilt_rad:Math.max(...frames.map(f=>Math.acos(Math.max(-1,Math.min(1,body(f).rotation[2][2]))))),feet,motor,positive_mechanical_work_j:work,native:{wall_s:c.wall_s,simulation_per_wall:frames.at(-1).time_s/c.wall_s,p95_transition_s:timings[Math.ceil(.95*timings.length)-1]}};
out.passes_noncollision_preliminary_screens=out.completed&&speed>=.0125&&out.stop_drift_m<=.003&&out.maximum_heading_error_rad<=.02&&out.maximum_tilt_rad<=.1&&feet.every(f=>f.motion_to_body_advance_ratio<=.05);
fs.writeFileSync(`${dir}/${name}.metrics.json`,JSON.stringify(out,null,2)+'\n');
console.log({name,speed_mm_s:out.measured_forward_speed_mm_s,stop_mm:out.stop_drift_m===null?null:out.stop_drift_m*1000,heading:out.maximum_heading_error_rad,tilt:out.maximum_tilt_rad,max_slip_ratio:Math.max(...feet.map(f=>f.motion_to_body_advance_ratio)),native:out.native,work_j:work});
