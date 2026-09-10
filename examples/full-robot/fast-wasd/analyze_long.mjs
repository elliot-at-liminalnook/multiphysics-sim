import fs from 'node:fs';import {recordedContactMotion} from '../../interactive/recorded_contact_motion.mjs';
const d='examples/full-robot/fast-wasd',rows=[];
for(const name of ['dropout','sustained']){
 const c=JSON.parse(fs.readFileSync(`${d}/${name}.native.json`)),frames=c.frames;
 const body=f=>f.poses.find(p=>p.name.includes('Chassis')),p=f=>body(f).position_m;
 const near=t=>frames.reduce((a,b)=>Math.abs(b.time_s-t)<Math.abs(a.time_s-t)?b:a);
 const distance=(a,b)=>Math.hypot(...p(near(b)).slice(0,2).map((x,i)=>x-p(near(a))[i]));
 let travel=0;for(let i=1;i<frames.length;i++)travel+=Math.hypot(...p(frames[i]).slice(0,2).map((x,k)=>x-p(frames[i-1])[k]));
 const feet=c.recording.config.policy.point_feedback.markers.map(m=>{
  const index=c.recording.scene.robot.links.findIndex(l=>l.name===m.link),samples=frames.map(f=>recordedContactMotion(f,index,m.link));let path=0;
  for(let i=1;i<samples.length;i++){let a=samples[i-1],b=samples[i];if(a.force>=1&&b.force>=1)path+=(b.time_s-a.time_s)*(a.speed+b.speed)/2}
  return {link:m.link,loaded_material_motion_m:path,motion_to_total_body_path_ratio:path/travel};
 });
 const windows=(name==='sustained'?[[1.4,20,1],[25,40,-1],[45,56,1]]:[[.8,3,1],[6.8,9,-1]]).map(([a,b,sign])=>{const x=near(a),y=near(b),h=Math.atan2(body(x).rotation[1][0],body(x).rotation[0][0]);return {start_s:a,end_s:b,directed_speed_mm_s:c.completed?1000*sign*((p(y)[0]-p(x)[0])*Math.cos(h)+(p(y)[1]-p(x)[1])*Math.sin(h))/(y.time_s-x.time_s):null}});
 const releases=name==='dropout'?[3,9]:[56];
 const stops=releases.map(t=>{const end=name==='dropout'&&t===3?6:frames.at(-1).time_s,tail=frames.filter(f=>f.time_s>=t&&f.time_s<end);const settled=tail.find((f,i)=>tail.slice(i).every(g=>Math.hypot(...body(g).velocity_m_s.slice(0,2))<=.001));return {request_or_packet_loss_s:t,permanently_below_1mm_s_at:settled?.time_s??null,travel_from_request_m:distance(t,end-.02),late_stop_drift_m:distance(t+.8,end-.02)}});
 const row={name,completed:c.completed,error:c.error,final_time_s:frames.at(-1).time_s,windows,stops,maximum_slip_ratio:Math.max(...feet.map(f=>f.motion_to_total_body_path_ratio)),maximum_tilt_rad:Math.max(...frames.map(f=>Math.acos(Math.max(-1,Math.min(1,body(f).rotation[2][2]))))),feet,total_horizontal_body_path_m:travel};rows.push(row);
}
fs.writeFileSync(`${d}/long-summary.json`,JSON.stringify({rows,scope:'Recorded shared-runtime motion. Dropout holds a stale nonzero velocity packet while physics and the Rust lease keep ticking. Hardware transport/sensors remain unbound; no sim-to-real accuracy claim.'},null,2)+'\n');console.log(rows.map(({name,completed,error,windows,stops,maximum_slip_ratio,maximum_tilt_rad})=>({name,completed,error,windows,stops,maximum_slip_ratio,maximum_tilt_rad})));
