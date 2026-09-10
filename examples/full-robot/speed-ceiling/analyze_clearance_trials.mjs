// Measurements of Rust recordings, using the existing material-point audit.
import fs from 'node:fs';import crypto from 'node:crypto';
import {recordedContactMotion} from '../../interactive/recorded_contact_motion.mjs';
const d='examples/full-robot/speed-ceiling',rows=[];
for(const def of JSON.parse(fs.readFileSync(`${d}/clearance-trials.json`)).rows){
 if(def.planning_exit!==0){rows.push({...def,accepted_screen:false,reason:'planning rejected',error:fs.readFileSync(`${def.prefix}.plan-error.txt`,'utf8')});continue;}
 const file=`${def.prefix}.native.json`;if(!fs.existsSync(file)||!fs.statSync(file).size)continue;
 let c;try{c=JSON.parse(fs.readFileSync(file));}catch{continue;}
 const frames=c.frames,body=f=>f.poses.find(p=>p.name.includes('Chassis'));
 const near=t=>frames.reduce((a,b)=>Math.abs(b.time_s-t)<Math.abs(a.time_s-t)?b:a);
 const select=frames.filter(f=>f.time_s>=1.4-1e-9&&f.time_s<=5.2+1e-9),mean=a=>a.reduce((s,x)=>s+x,0)/a.length;
 const mt=mean(select.map(f=>f.time_s)),mx=mean(select.map(f=>body(f).position_m[0]));
 const speed=select.reduce((s,f)=>s+(f.time_s-mt)*(body(f).position_m[0]-mx),0)/select.reduce((s,f)=>s+(f.time_s-mt)**2,0);
 const advance=body(frames.at(-1)).position_m[0]-body(frames[0]).position_m[0];
 const feet=c.recording.config.policy.task_observations.markers.map(m=>{
  const index=c.recording.scene.robot.links.findIndex(l=>l.name===m.link),samples=frames.map(f=>recordedContactMotion(f,index,m.link));let path=0;
  for(let i=1;i<samples.length;i++){const a=samples[i-1],b=samples[i];if(a.force>=1&&b.force>=1)path+=(b.time_s-a.time_s)*(a.speed+b.speed)/2;}
  return {link:m.link,loaded_material_path_m:path,slip_ratio:path/advance};
 });
 const motors=c.metadata.coordinate_names.map((name,i)=>({name,
  peak_rate_rad_s:Math.max(...frames.map(f=>Math.abs(f.motor_readings[i].gear_speed_rad_s))),
  peak_torque_nm:Math.max(...frames.map(f=>Math.abs(f.motor_readings[i].shaft_torque_nm)))}));
 const maximum_slip_ratio=Math.max(...feet.map(f=>f.slip_ratio));
 const maximum_tilt_rad=Math.max(...frames.map(f=>Math.acos(Math.max(-1,Math.min(1,body(f).rotation[2][2])))));
 const stopped_speed_m_s=Math.hypot(...body(frames.at(-1)).velocity_m_s.slice(0,2));
 const row={...def,completed:c.completed,error:c.error,simulated_s:frames.at(-1).time_s,
  speed_m_s:c.completed?speed:null,maximum_slip_ratio,maximum_tilt_rad,stopped_speed_m_s,
  release_travel_m:c.completed?Math.hypot(...body(frames.at(-1)).position_m.slice(0,2).map((v,i)=>v-body(near(5.2)).position_m[i])):null,
  feet,motors,capture_sha256:crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex'),
  accepted_screen:c.completed&&speed>=def.speed*.95&&maximum_slip_ratio<=.05&&maximum_tilt_rad<=.1&&stopped_speed_m_s<=.001};
 rows.push(row);
}
fs.writeFileSync(`${d}/clearance-summary.json`,JSON.stringify({scope:'Development screen only; full recorded geometry, steering, sustained and finer-timestep checks still required. Loaded path uses material velocity with normal force >=1 N at both sample endpoints.',rows},null,2)+'\n');
console.log(rows.map(({name,completed,speed_m_s,maximum_slip_ratio,stopped_speed_m_s,accepted_screen,reason})=>({name,completed,speed_m_s,maximum_slip_ratio,stopped_speed_m_s,accepted_screen,reason})));
