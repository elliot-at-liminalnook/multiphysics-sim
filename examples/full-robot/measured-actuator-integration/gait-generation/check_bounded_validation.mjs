import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
const [mode,basePath,otherPath,protocolPath,out]=process.argv.slice(2);
if(!out)throw Error('check_bounded_validation.mjs replay|half-step|stop BASE OTHER PROTOCOL NEW_REPORT');
const read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const a=read(basePath),b=read(otherPath),p=read(protocolPath);
assert.equal(a.recording.seed,b.recording.seed);assert.equal(a.recording.scene.robot.source.cad_sha256,b.recording.scene.robot.source.cad_sha256);
const end=c=>c.transitions.at(-1),body=(c,f)=>f.poses.find(x=>x.name===c.task.speed.body_link).position_m;
const distance=(x,y)=>Math.hypot(...x.map((v,i)=>v-y[i]));
const checks={completed:a.completed&&b.completed&&!a.error&&!b.error,not_fallen:b.transitions.every(t=>!t.speed.fallen)};let metrics={};
if(mode==='replay'){
 const clean=c=>JSON.parse(JSON.stringify(c,(key,value)=>['stepping_wall_s','transition_wall_s','wall_s'].includes(key)?undefined:value));
 checks.frames_exact=JSON.stringify(clean(a.frames))===JSON.stringify(clean(b.frames));checks.transitions_exact=JSON.stringify(a.transitions)===JSON.stringify(b.transitions);
}else if(mode==='half-step'){
 assert.equal(a.recording.config.step_s/2,b.recording.config.step_s);assert.equal(end(a).time_s,end(b).time_s);
 metrics={net_distance_difference_m:Math.abs(end(a).speed.net_distance_m-end(b).speed.net_distance_m),
 body_position_difference_m:distance(body(a,a.frames.at(-1)),body(b,b.frames.at(-1))),
 body_up_z_difference:Math.abs(end(a).speed.body_up_z-end(b).speed.body_up_z),
 maximum_actuated_angle_difference_rad:Math.max(...a.task.observations.map((o,i)=>o.source.kind==='coordinate_position'?Math.abs(end(a).observations[i]-end(b).observations[i]):0))};
 checks.net_distance=metrics.net_distance_difference_m<=p.numerical.maximum_absolute_net_distance_difference_m;
 checks.body_position=metrics.body_position_difference_m<=p.numerical.maximum_endpoint_body_position_difference_m;
 checks.body_up=metrics.body_up_z_difference<=p.numerical.maximum_endpoint_body_up_z_difference;
 checks.actuated_angles=metrics.maximum_actuated_angle_difference_rad<=p.numerical.maximum_endpoint_actuated_joint_difference_rad;
}else if(mode==='stop'){
 const tail=b.frames.filter(f=>f.time_s>=9),origin=body(b,tail[0]);metrics.maximum_final_second_drift_m=Math.max(...tail.map(f=>distance(body(b,f),origin)));
 const afterStop=b.frames.filter(f=>f.time_s>=p.stop.moving_s),stopOrigin=body(b,afterStop[0]);
 metrics.maximum_displacement_after_stop_request_m=Math.max(...afterStop.map(f=>distance(body(b,f),stopOrigin)));
 checks.horizon=end(b).time_s===p.stop.moving_s+p.stop.stopped_s;
 checks.drift=metrics.maximum_final_second_drift_m<=p.stop.final_one_second_maximum_net_drift_m;
 const i=b.contract.actions.findIndex(a=>a.name==='command.forward_speed');assert(i>=0);
 checks.stop_command=b.frames.filter(f=>f.time_s>7).every(f=>f.policy_inputs[i]===0);
}else throw Error('Unknown mode');
const report={mode,inputs:[basePath,otherPath,protocolPath].map(path=>({path,sha256:sha(path)})),metrics,checks,pass:Object.values(checks).every(Boolean)};
fs.writeFileSync(out,JSON.stringify(report,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(report));
