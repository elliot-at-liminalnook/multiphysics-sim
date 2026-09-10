// Measurements from Rust recordings; no alternate physics implementation.
import fs from 'node:fs';import crypto from 'node:crypto';
import {recordedContactMotion} from '../../interactive/recorded_contact_motion.mjs';
const d='examples/full-robot/fast-wasd',rows=[];
for(const {name,...definition} of JSON.parse(fs.readFileSync(`${d}/speed-trials.json`)).definitions){
 const bytes=fs.readFileSync(`${d}/${name}.native.json`),c=JSON.parse(bytes),frames=c.frames;
 const body=f=>f.poses.find(p=>p.name.includes('Chassis')),p=f=>body(f).position_m;
 const near=t=>frames.reduce((a,b)=>Math.abs(b.time_s-t)<Math.abs(a.time_s-t)?b:a);
 const selected=frames.filter(f=>f.time_s>=1.2-1e-9&&f.time_s<=5.2+1e-9);
 const mean=a=>a.reduce((s,x)=>s+x,0)/a.length;
 const mt=mean(selected.map(f=>f.time_s)),mx=mean(selected.map(f=>p(f)[0]));
 const speed=selected.reduce((s,f)=>s+(f.time_s-mt)*(p(f)[0]-mx),0)/selected.reduce((s,f)=>s+(f.time_s-mt)**2,0);
 const net=Math.hypot(...p(frames.at(-1)).slice(0,2).map((x,i)=>x-p(frames[0])[i]));
 const feet=c.recording.config.policy.point_feedback.markers.map(m=>{
  const index=c.recording.scene.robot.links.findIndex(l=>l.name===m.link),samples=frames.map(f=>recordedContactMotion(f,index,m.link));let path=0;
  for(let i=1;i<samples.length;i++){let a=samples[i-1],b=samples[i];if(a.force>=1&&b.force>=1)path+=(b.time_s-a.time_s)*(a.speed+b.speed)/2}
  return {link:m.link,loaded_material_motion_m:path,motion_to_net_body_advance_ratio:path/net};
 });
 const motor=c.metadata.coordinate_names.map((name,i)=>({name,maximum_speed_rad_s:Math.max(...frames.map(f=>Math.abs(f.motor_readings[i].gear_speed_rad_s))),maximum_torque_nm:Math.max(...frames.map(f=>Math.abs(f.motor_readings[i].shaft_torque_nm))),angle_span_rad:Math.max(...frames.map(f=>f.joint_positions[c.metadata.joint_indices[i]]))-Math.min(...frames.map(f=>f.joint_positions[c.metadata.joint_indices[i]]))}));
 let work=0;for(let i=1;i<frames.length;i++)for(let j=0;j<motor.length;j++){const power=f=>Math.max(0,f.motor_readings[j].gear_speed_rad_s*f.motor_readings[j].shaft_torque_nm);work+=(frames[i].time_s-frames[i-1].time_s)*(power(frames[i])+power(frames[i-1]))/2}
 const timings=[...c.transition_wall_s].sort((a,b)=>a-b);
 const row={name,...definition,completed:c.completed,error:c.error,capture_sha256:crypto.createHash('sha256').update(bytes).digest('hex'),simulated_s:frames.at(-1).time_s,measured_forward_speed_mm_s:c.completed?1000*speed:null,stop_drift_m:c.completed?Math.hypot(...p(frames.at(-1)).slice(0,2).map((x,i)=>x-p(near(5.2))[i])):null,maximum_tilt_rad:Math.max(...frames.map(f=>Math.acos(Math.max(-1,Math.min(1,body(f).rotation[2][2]))))),maximum_heading_rad:Math.max(...frames.map(f=>Math.abs(Math.atan2(body(f).rotation[1][0],body(f).rotation[0][0])))),maximum_slip_ratio:Math.max(...feet.map(f=>f.motion_to_net_body_advance_ratio)),feet,motor,positive_mechanical_work_j:work,native:{wall_s:c.wall_s,simulation_per_wall:frames.at(-1).time_s/c.wall_s,p95_transition_s:timings[Math.ceil(.95*timings.length)-1]}};
 rows.push(row);fs.writeFileSync(`${d}/${name}.metrics.json`,JSON.stringify(row,null,2)+'\n');
}
fs.writeFileSync(`${d}/speed-summary.json`,JSON.stringify({rows,scope:'6 s live-command development screening; independent geometry, sustained steering and browser checks remain separate.'},null,2)+'\n');
console.log(rows.map(({name,completed,simulated_s,measured_forward_speed_mm_s,stop_drift_m,maximum_slip_ratio,maximum_tilt_rad,motor,native})=>({name,completed,simulated_s,measured_forward_speed_mm_s,stop_drift_m,maximum_slip_ratio,maximum_tilt_rad,peak_motor_speed:Math.max(...motor.map(m=>m.maximum_speed_rad_s)),native})));
