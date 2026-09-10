// Measurements only: project recorded motion onto the declared travel heading.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {recordedContactMotion} from '../../interactive/recorded_contact_motion.mjs';
const [prefix,output,commandSpeedArg]=process.argv.slice(2);
assert(prefix&&output&&!fs.existsSync(output),'usage: analyze_controller_screen.mjs prefix fresh-summary.json');
const file=prefix+'.native.json',c=JSON.parse(fs.readFileSync(file)),frames=c.frames;
assert(c.completed&&c.error===null&&Math.abs(frames.at(-1).time_s-8)<1e-9,'complete standard eight-second screen required');
const p=c.recording.scene.controller.parameters,headingOffset=p.travel_heading_offset_rad??0;
const commandSpeed=commandSpeedArg===undefined?p.nominal_speed_m_s:Number(commandSpeedArg);
assert(Number.isFinite(commandSpeed)&&commandSpeed>0,'positive measured command speed required');
if(commandSpeedArg!==undefined){
 const index=c.recording.scene.controller.inputs.findIndex(i=>i.name==='command.forward_speed');
 assert(index>=0);
 for(const [start,end,sign] of [[1.4,3,1],[4.8,6.4,-1]]){
  const selected=frames.filter(f=>f.time_s>=start&&f.time_s<end);
  assert(selected.length>2&&selected.every(f=>f.policy_inputs[index]===sign*commandSpeed),
   'speed comparison must match the recorded command throughout each measurement window');
 }
}
const body=f=>f.poses.find(p=>p.name.includes('Chassis')),pos=f=>body(f).position_m;
const yaw=f=>Math.atan2(body(f).rotation[1][0],body(f).rotation[0][0]);
const near=t=>frames.reduce((a,b)=>Math.abs(b.time_s-t)<Math.abs(a.time_s-t)?b:a);
const distance=(a,b)=>Math.hypot(...pos(near(b)).slice(0,2).map((v,i)=>v-pos(near(a))[i]));
const segments=[[1.4,3,1],[4.8,6.4,-1]].map(([start,end,sign])=>{
 const a=near(start),b=near(end),h=yaw(a)+headingOffset,dt=b.time_s-a.time_s;
 const dx=pos(b)[0]-pos(a)[0],dy=pos(b)[1]-pos(a)[1];
 const forward=sign*(dx*Math.cos(h)+dy*Math.sin(h))/dt,lateral=sign*(-dx*Math.sin(h)+dy*Math.cos(h))/dt;
 return {window_s:[start,end],command_sign:sign,speed_along_heading_m_s:forward,lateral_speed_m_s:lateral,
  direction_error_rad:Math.atan2(lateral,forward)};
});
const stops=[[3,4],[6.4,8]].map(([start,end])=>{
 const tail=frames.filter(f=>f.time_s>=start&&f.time_s<end);let settled=null;
 for(let i=tail.length-1;i>=0;i--){if(Math.hypot(...body(tail[i]).velocity_m_s.slice(0,2))>.001)break;settled=tail[i].time_s;}
 return {window_s:[start,end],settled_after_s:settled===null?null:settled-start,release_travel_m:distance(start,end-.02),late_drift_m:distance(start+.8,end-.02)};
});
let path=0;for(let i=1;i<frames.length;i++)path+=Math.hypot(...pos(frames[i]).slice(0,2).map((v,j)=>v-pos(frames[i-1])[j]));
const feet=c.recording.config.policy.task_observations.markers.map(m=>{
 const index=c.recording.scene.robot.links.findIndex(l=>l.name===m.link),samples=frames.map(f=>recordedContactMotion(f,index,m.link));let loaded=0;
 for(let i=1;i<samples.length;i++){const a=samples[i-1],b=samples[i];if(a.force>=1&&b.force>=1)loaded+=(b.time_s-a.time_s)*(a.speed+b.speed)/2;}
 return {link:m.link,loaded_material_path_m:loaded,ratio_to_total_body_path:loaded/path};
});
const tilt=Math.max(...frames.map(f=>Math.acos(Math.max(-1,Math.min(1,body(f).rotation[2][2])))));
const slip=Math.max(...feet.map(f=>f.ratio_to_total_body_path));
const control=segments.every(s=>Math.abs(s.speed_along_heading_m_s/commandSpeed-1)<=.05&&Math.abs(s.direction_error_rad)<=.1)
 &&tilt<=.1&&stops.every(s=>s.settled_after_s!==null&&s.settled_after_s<=.8+1e-9&&s.late_drift_m<=.003);
const summary={completed:c.completed,physics_step_s:c.recording.config.step_s,frames:frames.length,wall_s:c.wall_s,
 travel_heading_offset_rad:headingOffset,command_speed_m_s:commandSpeed,segments,stops,maximum_tilt_rad:tilt,
 feet,total_body_path_m:path,maximum_slip_ratio:slip,passed_control_checks:control,passed_contact_quality:slip<=.05,
 capture_sha256:crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex'),
 scope:'Standard eight-second forward/stop/reverse/stop screen. Signed travel is projected onto chassis yaw plus the explicitly declared travel offset; orthogonal drift is reported. Control gates: speed ±5%, travel-heading error <=0.1 rad, tilt <=0.1 rad, stops below 1 mm/s within 0.8 s and <=3 mm late drift. Contact quality separately requires <=5% loaded material slip. No clearance, collision, sustained, steering, timestep or hardware-transfer certificate.'};
fs.writeFileSync(output,JSON.stringify(summary,null,2)+'\n',{flag:'wx'});console.log({segments,stops,tilt,slip,control});
