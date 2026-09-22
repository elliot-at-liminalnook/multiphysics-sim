// Read-only statistics over saved Rust observations; no physics or control here.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
const [path,protocolPath,output,geometryPath]=process.argv.slice(2);
if(!output)throw Error('summarize_bounded_capture.mjs CAPTURE PROTOCOL NEW_REPORT [GEOMETRY_AUDIT]');
const read=p=>JSON.parse(fs.readFileSync(p));const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const c=read(path),protocol=read(protocolPath),limits=protocol.finalist;
assert.equal(c.kind,'sampled_environment_capture');assert.equal(c.frames.length,c.transitions.length);
const dt=c.task.period_s,q=180/Math.PI;
for(let i=0;i<c.frames.length;i++){assert.equal(c.frames[i].time_s,c.transitions[i].time_s);if(i)assert(Math.abs(c.frames[i].time_s-c.frames[i-1].time_s-dt)<1e-8);}
const frames=c.frames,transitions=c.transitions,begin=frames.findIndex(f=>f.time_s>=limits.after_startup_s);
assert(begin>=0,'Capture shorter than startup exclusion');
const maxAbs=a=>Math.max(...a.map(Math.abs));const rms=a=>Math.sqrt(a.reduce((s,v)=>s+v*v,0)/a.length);
const motors=c.recording.config.motors.target_coordinates.map((coordinate,k)=>{
    const i=c.task.observations.findIndex(o=>o.source.kind==='coordinate_position'&&o.source.coordinate===coordinate);assert(i>=0);
    const errors=frames.slice(begin).map((f,j)=>(f.servo_targets_rad[k]-transitions[j+begin].observations[i])*q);
    const commands=frames.map(f=>f.servo_targets_rad[k]);
    const velocities=commands.slice(1).map((v,j)=>(v-commands[j])/dt*q);
    const accelerations=velocities.slice(1).map((v,j)=>(v-velocities[j])/dt);
    // Exclude the initial CAD reference -> sensor-origin initialization from derivative checks.
    const speed=maxAbs(velocities.slice(1)),acceleration=maxAbs(accelerations.slice(1));
    return {coordinate,rms_tracking_degrees:rms(errors),peak_tracking_degrees:maxAbs(errors),
        maximum_command_speed_degrees_s:speed,maximum_command_acceleration_degrees_s2:acceleration,
        saturated_fraction:frames.slice(begin).filter(f=>Math.abs(f.servo_commands[k])>=0.999999).length/(frames.length-begin),
        tracking_pass:rms(errors)<=limits.maximum_each_motor_rms_tracking_degrees&&maxAbs(errors)<=limits.maximum_each_motor_peak_tracking_degrees,
        command_bounds_pass:speed<=limits.maximum_command_speed_degrees_s+1e-7&&acceleration<=limits.maximum_command_acceleration_degrees_s2+1e-6};
});
const last=transitions.at(-1),minimumUp=Math.min(...transitions.map(t=>t.speed.body_up_z));
const checks={completed:c.completed&&!c.error&&!c.recording.failure,not_fallen:transitions.every(t=>!t.speed.fallen),
    upright:minimumUp>=limits.minimum_body_up_z,command_bounds:motors.every(m=>m.command_bounds_pass),tracking:motors.every(m=>m.tracking_pass),
    finalist_horizon:Math.abs(last.time_s-limits.horizon_s)<1e-8,travel:last.speed.net_distance_m>=limits.minimum_net_distance_m};
let geometry=null;
if(geometryPath){
    const g=read(geometryPath);assert.equal(g.source.cad_sha256,c.recording.scene.robot.source.cad_sha256);
    assert.equal(g.frames.length,frames.length);assert(g.frames.every((f,i)=>f.time_s===frames[i].time_s));
    const feet=c.recording.config.policy.task_observations.markers.map(m=>m.link);
    geometry={path:geometryPath,sha256:sha(geometryPath),feet:feet.map(link=>{
        const heights=g.frames.slice(begin).map(f=>f.floor_clearances.find(x=>x.link===link)?.minimum_clearance_m);assert(heights.every(Number.isFinite));
        let excursions=0,up=false;for(const h of heights){if(h>limits.minimum_each_foot_peak_clearance_m&&!up){excursions++;up=true;}else if(h<=0.001){up=false;}}
        return {link,maximum_clearance_m:Math.max(...heights),minimum_clearance_m:Math.min(...heights),clearance_excursions:excursions};
    }),maximum_inter_link_penetration_m:g.frames.reduce((maximum,f)=>(f.inter_link_penetrations??[]).reduce((m,p)=>Math.max(m,p.penetration_m),maximum),0)};
    checks.foot_lift=geometry.feet.every(f=>f.maximum_clearance_m>=limits.minimum_each_foot_peak_clearance_m&&f.clearance_excursions>=limits.minimum_each_foot_clearance_excursions);
    checks.sampled_collision=geometry.maximum_inter_link_penetration_m<=limits.maximum_sampled_inter_link_penetration_m;
}
const report={simulation_only:true,capture:{path,sha256:sha(path)},protocol:{path:protocolPath,sha256:sha(protocolPath)},
    horizon_s:last.time_s,wall_s:c.wall_s,seed:c.recording.seed,controller_values:c.motion_parameters?.values,
    net_distance_m:last.speed.net_distance_m,net_speed_m_s:last.speed.net_speed_m_s,minimum_body_up_z:minimumUp,motors,geometry,checks,
    passes_all_finalist_checks:!!geometry&&Object.values(checks).every(Boolean),
    limitations:'Provisional CAD actuator scenario. Tracking uses same-time physical joint and held motor target, after 1 s startup. Sampled geometry cannot certify continuous clearance. Numerical and stop checks are separate.'};
fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({horizon_s:report.horizon_s,net_speed_m_s:report.net_speed_m_s,checks,worst_motor_rms_degrees:Math.max(...motors.map(m=>m.rms_tracking_degrees))}));
