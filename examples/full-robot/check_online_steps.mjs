// Validate executed swings using CAD contact surfaces through the shared Rust tool.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [capturePath,output]=process.argv.slice(2);assert(capturePath&&output,'usage: check_online_steps.mjs capture.json output-directory');
const read=p=>JSON.parse(readFileSync(p)),capture=read(capturePath),config=capture.recording?.config;
assert(capture.completed&&!capture.error&&config&&capture.recording?.scene);mkdirSync(output,{recursive:true});
// Inspect the exact recorded recipe, including controller variants and overrides.
const scenePath=`${output}/recorded.scene.json`;
writeFileSync(scenePath,JSON.stringify(capture.recording.scene)+'\n');
writeFileSync(`${output}/recorded.config.json`,JSON.stringify(config)+'\n');
const markers=config.policy.point_feedback.markers,phases=new Map();
for(const f of capture.frames){const r=f.policy?.step_reference?.reference;if(!r)continue;
 const p=phases.get(r.step)||{step:r.step,foot:r.foot,start:Infinity,end:-Infinity,returned:false};
 if(r.foot!=null)p.foot=r.foot;
 if(r.phase==='raise')p.start=Math.min(p.start,f.time_s);
 if(r.phase==='lower')p.end=Math.max(p.end,f.time_s);
 if(r.phase==='return'||r.phase==='settle')p.returned=true;
 phases.set(r.step,p);
}
const lifts=[];
for(const p of phases.values())if(p.returned){
 assert(Number.isFinite(p.start)&&Number.isFinite(p.end));
 const req={start_s:p.start,end_s:p.end,maximum_sample_gap_s:.020001,qualifying_duration_s:.2,swing_link:markers[p.foot].link,minimum_clearance_m:.001,maximum_swing_force_n:.1,minimum_support_forces_n:Object.fromEntries(markers.filter((_,i)=>i!==p.foot).map(m=>[m.link,1]))};
 const name=`${output}/step-${p.step}`;writeFileSync(`${name}.requirements.json`,JSON.stringify(req));
 const text=execFileSync('target/release/examples/evaluate_lift',[scenePath,capturePath,`${name}.requirements.json`],{encoding:'utf8',maxBuffer:32*1024*1024});
 writeFileSync(`${name}.report.json`,text);lifts.push({step:p.step,foot:p.foot,...JSON.parse(text).report});
}
const body=f=>f.poses.find(p=>p.name===config.policy.body_feedback.reference_link),final=capture.frames.at(-1),r=final.policy.step_reference.reference,b=body(final);
const internal=capture.frames.reduce((n,f)=>n+f.contacts.filter(c=>c.other!=null).length,0);
const tilt=Math.max(...capture.frames.map(f=>Math.acos(Math.max(-1,Math.min(1,body(f).rotation[2][2])))));
const error=Math.hypot(...b.position_m.map((v,i)=>v-r.body_world_m[i]));
const yaw=Math.atan2(b.rotation[1][0],b.rotation[0][0]);const yawError=Math.abs(Math.atan2(Math.sin(yaw-r.yaw_rad),Math.cos(yaw-r.yaw_rad)));
const budgets={minimum_transfers:4,maximum_final_body_error_m:.001,maximum_body_tilt_rad:.01,maximum_final_yaw_error_rad:.005};
const passed=lifts.length>=budgets.minimum_transfers&&lifts.every(l=>l.passed)&&internal===0&&tilt<=budgets.maximum_body_tilt_rad&&error<=budgets.maximum_final_body_error_m&&yawError<=budgets.maximum_final_yaw_error_rad&&r.phase==='idle';
const result={version:1,passed,capture:{path:capturePath,sha256:createHash('sha256').update(readFileSync(capturePath)).digest('hex')},simulated_s:final.time_s,lifts,final_phase:r.phase,body_advance_world_m:b.position_m.map((v,i)=>v-body(capture.frames[0]).position_m[i]),final_yaw_rad:yaw,final_target_yaw_rad:r.yaw_rad,final_yaw_error_rad:yawError,final_body_error_m:error,maximum_body_tilt_rad:tilt,sampled_internal_contacts:internal,budgets,
 scope:'Initial flat-floor commissioning with ideal observations. Actual clearance/support samples, endpoint tracking and stop checked; not between-sample impact accuracy, terrain robustness, hardware calibration or sustained realtime acceptance.'};
writeFileSync(`${output}/summary.json`,JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result,null,2));assert(passed);
