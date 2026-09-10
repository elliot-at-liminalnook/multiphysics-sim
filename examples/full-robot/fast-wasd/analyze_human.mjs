import fs from 'node:fs';import {recordedContactMotion} from '../../interactive/recorded_contact_motion.mjs';
const d='examples/full-robot/fast-wasd',read=p=>JSON.parse(fs.readFileSync(`${d}/${p}.json`));
const plan=read('human-trials'),rows=[];
for(const definition of plan.cases){
 const {name}=definition,c=read(`${name}.native`),frames=c.frames;
 const body=f=>f.poses.find(p=>p.name.includes('Chassis')),p=f=>body(f).position_m,yaw=f=>Math.atan2(body(f).rotation[1][0],body(f).rotation[0][0]);
 const near=t=>frames.reduce((a,b)=>Math.abs(b.time_s-t)<Math.abs(a.time_s-t)?b:a);
 const windows=[[1.4,6,1],[11,16,-1]].map(([a,b,sign])=>{let x=near(a),y=near(b),h=yaw(x);return {start_s:a,end_s:b,directed_speed_mm_s:c.completed?sign*1000*((p(y)[0]-p(x)[0])*Math.cos(h)+(p(y)[1]-p(x)[1])*Math.sin(h))/(y.time_s-x.time_s):null}});
 let totalTravel=0;for(let i=1;i<frames.length;i++)totalTravel+=Math.hypot(p(frames[i])[0]-p(frames[i-1])[0],p(frames[i])[1]-p(frames[i-1])[1]);
 const feet=c.recording.config.policy.point_feedback.markers.map(m=>{
  const index=c.recording.scene.robot.links.findIndex(l=>l.name===m.link),samples=frames.map(f=>recordedContactMotion(f,index,m.link));let path=0;
  for(let i=1;i<samples.length;i++){let a=samples[i-1],b=samples[i];if(a.force>=1&&b.force>=1)path+=(b.time_s-a.time_s)*(a.speed+b.speed)/2}
  return {link:m.link,loaded_material_motion_m:path,motion_to_total_body_path_ratio:path/totalTravel};
 });
 const times=[...c.transition_wall_s].sort((a,b)=>a-b),row={...definition,completed:c.completed,error:c.error,final_time_s:frames.at(-1).time_s,windows,turn_rad:c.completed?yaw(near(10))-yaw(near(6)):null,stop_drift_after_transfer_m:c.completed?Math.hypot(p(near(20))[0]-p(near(16.4))[0],p(near(20))[1]-p(near(16.4))[1]):null,stop_travel_from_release_m:c.completed?Math.hypot(p(near(20))[0]-p(near(16))[0],p(near(20))[1]-p(near(16))[1]):null,maximum_tilt_rad:Math.max(...frames.map(f=>Math.acos(Math.max(-1,Math.min(1,body(f).rotation[2][2]))))),total_horizontal_body_path_m:totalTravel,maximum_slip_ratio:Math.max(...feet.map(f=>f.motion_to_total_body_path_ratio)),feet,native:{wall_s:c.wall_s,simulation_per_wall:frames.at(-1).time_s/c.wall_s,p95_transition_s:times[Math.ceil(.95*times.length)-1]}};
 rows.push(row);fs.writeFileSync(`${d}/${name}.metrics.json`,JSON.stringify(row,null,2)+'\n');
}
fs.writeFileSync(`${d}/human-summary.json`,JSON.stringify({rows,scope:'20 s forward/walking-turn/reverse/stop. Slip denominator is total body path because reversal cancels net displacement. Stopping reports both travel since release and residual drift after 0.4 s transfer allowance. Independent geometry and hardware transfer remain separate.'},null,2)+'\n');
console.log(rows.map(({name,completed,windows,turn_rad,stop_drift_after_transfer_m,stop_travel_from_release_m,maximum_slip_ratio,native})=>({name,completed,windows,turn_rad,stop_drift_after_transfer_m,stop_travel_from_release_m,maximum_slip_ratio,native})));
