import fs from 'node:fs';import crypto from 'node:crypto';
import {recordedContactMotion} from '../../interactive/recorded_contact_motion.mjs';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p)),rows=[],human={};
const body=f=>f.poses.find(p=>p.name.includes('Chassis')),pos=f=>body(f).position_m,yaw=f=>Math.atan2(body(f).rotation[1][0],body(f).rotation[0][0]);
for(const def of read(process.argv[2]??`${d}/validation-cases.json`).rows){
 const file=`${def.prefix}.native.json`;if(!fs.existsSync(file)||!fs.statSync(file).size)continue;
 let c;try{c=read(file);}catch{continue;}
 const frames=c.frames,near=t=>frames.reduce((a,b)=>Math.abs(b.time_s-t)<Math.abs(a.time_s-t)?b:a);
 const distance=(a,b)=>Math.hypot(...pos(near(b)).slice(0,2).map((v,i)=>v-pos(near(a))[i]));
 let path=0;for(let i=1;i<frames.length;i++)path+=Math.hypot(...pos(frames[i]).slice(0,2).map((v,j)=>v-pos(frames[i-1])[j]));
 const windows=(def.kind==='human'?[[1.4,6,1],[11,16,-1]]:def.kind==='sustained'?[[1.4,20,1],[25,40,-1],[45,56,1]]:[[.8,3,1],[6.8,9,-1]]).map(([a,b,sign])=>{
  const first=near(a),last=near(b),h=yaw(first)+(def.travel_heading_offset_rad??0);
  const dx=pos(last)[0]-pos(first)[0],dy=pos(last)[1]-pos(first)[1],dt=last.time_s-first.time_s;
  const speed=c.completed?sign*(dx*Math.cos(h)+dy*Math.sin(h))/dt:null;
  const lateral=c.completed?sign*(-dx*Math.sin(h)+dy*Math.cos(h))/dt:null;
  return {start_s:a,end_s:b,speed_m_s:speed,lateral_speed_m_s:lateral,
   direction_error_rad:c.completed?Math.atan2(lateral,speed):null};
 });
 const feet=c.recording.config.policy.task_observations.markers.map(m=>{
  const index=c.recording.scene.robot.links.findIndex(l=>l.name===m.link),samples=frames.map(f=>recordedContactMotion(f,index,m.link));let loaded=0;
  for(let i=1;i<samples.length;i++){const a=samples[i-1],b=samples[i];if(a.force>=1&&b.force>=1)loaded+=(b.time_s-a.time_s)*(a.speed+b.speed)/2;}
  return {link:m.link,loaded_material_path_m:loaded,ratio_to_total_body_path:loaded/path};
 });
 const stops=(def.kind==='human'?[[16,20]]:def.kind==='sustained'?[[56,60]]:[[3,6],[9,12]]).map(([start,end])=>{
  const tail=frames.filter(f=>f.time_s>=start&&f.time_s<end);let settled=null;
  for(let i=tail.length-1;i>=0;i--){if(Math.hypot(...body(tail[i]).velocity_m_s.slice(0,2))>.001)break;settled=tail[i].time_s;}
  return {request_or_packet_loss_s:start,permanently_below_1mm_s_at:settled,release_travel_m:distance(start,end-.02),late_drift_m:distance(start+.8,end-.02)};
 });
 const slip=Math.max(...feet.map(f=>f.ratio_to_total_body_path)),tilt=Math.max(...frames.map(f=>Math.acos(Math.max(-1,Math.min(1,body(f).rotation[2][2])))));
 const turn=def.kind==='human'?yaw(near(10))-yaw(near(6)):null;
 // These human schedules request 0.06 rad/s for four seconds. Preserve actual
 // A/D response as well as forward speed; this is a development tolerance.
 const turnPassed=turn===null||Math.abs(turn-.24)<=.06;
 // Simulation timestamps such as 3.8000000000000003 can differ from the
 // decimal 0.8 s boundary by roundoff; 1 ns is far below any reporting step.
 const headingPassed=c.completed&&windows.every(w=>Math.abs(w.direction_error_rad)<=.1);
 const controlPassed=c.completed&&headingPassed&&windows.every(w=>w.speed_m_s>=def.command_speed_m_s*.95&&w.speed_m_s<=def.command_speed_m_s*1.05)&&tilt<=.1&&turnPassed&&stops.every(s=>s.permanently_below_1mm_s_at!==null&&s.permanently_below_1mm_s_at-s.request_or_packet_loss_s<=.8+1e-9&&s.late_drift_m<=.003);
 const row={...def,completed:c.completed,error:c.error,simulated_s:frames.at(-1).time_s,windows,stops,feet,total_body_path_m:path,
  maximum_slip_ratio:slip,maximum_tilt_rad:tilt,turn_rad:turn,passed_turn_response:turnPassed,passed_travel_heading:headingPassed,
  passed_control_checks:controlPassed,passed_contact_quality:slip<=.05,
  passed_motion_checks:controlPassed&&slip<=.05,
  capture_sha256:crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')};
 // Timestep comparisons need only body positions/times. Retaining complete
 // captures here otherwise grows memory with every new experiment family.
 rows.push(row);if(def.kind==='human')human[def.name]=frames.map(f=>({time_s:f.time_s,position_m:pos(f)}));
}
const comparisons=[];
const comparisonReferenceStep=process.argv[4]===undefined?.00125:Number(process.argv[4]);
if(!Number.isFinite(comparisonReferenceStep)||comparisonReferenceStep<=0)throw Error('positive comparison reference timestep required');
for(const ra of rows.filter(r=>r.kind==='human'&&r.step_s===comparisonReferenceStep)){
 const candidates=ra.family?rows.filter(r=>r.family===ra.family&&r.kind==='human'&&r.name!==ra.name):rows.filter(r=>['human-5ms','human-0p625ms'].includes(r.name));
 for(const rb of candidates){
 const name=rb.name,a=human[ra.name],b=human[name];
 if(a.length!==b.length)throw Error('capture sample mismatch');
 let maximum_body_difference_m=0;
 for(let i=0;i<a.length;i++){
  if(Math.abs(a[i].time_s-b[i].time_s)>1e-9)throw Error('capture time mismatch');
  maximum_body_difference_m=Math.max(maximum_body_difference_m,Math.hypot(...a[i].position_m.map((v,j)=>v-b[i].position_m[j])));
 }
 const maximum_speed_fraction=Math.max(...ra.windows.map((w,i)=>Math.abs(w.speed_m_s-rb.windows[i].speed_m_s)/w.speed_m_s));
 comparisons.push({reference:ra.name,candidate:name,maximum_body_difference_m,maximum_speed_fraction,
  absolute_slip_ratio_difference:Math.abs(ra.maximum_slip_ratio-rb.maximum_slip_ratio),
  both_motion_checks_passed:ra.passed_motion_checks&&rb.passed_motion_checks,
  passed:maximum_body_difference_m<=.003&&maximum_speed_fraction<=.02});
}}
fs.writeFileSync(process.argv[3]??`${d}/validation-summary.json`,JSON.stringify({rows,comparisons,scope:'Command-driven native recordings. Control gates require ±5% speed tracking, <=0.1 rad travel-heading error relative to the declared chassis offset at each window start, <=0.1 rad tilt, human-schedule yaw change 0.24±0.06 rad, settling below 1 mm/s within 0.8 s and <=3 mm late drift. Contact quality separately requires <=5% loaded material slip; combined motion checks require both. These development thresholds are not physical speed limits. Collision, periodic lift and browser checks remain separate.'},null,2)+'\n');
console.log(rows.map(({name,completed,windows,maximum_slip_ratio,turn_rad,stops,passed_motion_checks})=>({name,completed,windows,maximum_slip_ratio,turn_rad,stops,passed_motion_checks})),comparisons);
