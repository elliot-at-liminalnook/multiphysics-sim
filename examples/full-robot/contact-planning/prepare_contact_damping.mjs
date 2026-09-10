// Reuse the shared Rust contact-velocity observation/Jacobian correction.
import fs from 'node:fs';import crypto from 'node:crypto';import assert from 'node:assert/strict';
const [source,prefix]=process.argv.slice(2),d='examples/full-robot/contact-planning';
assert(source&&prefix&&source!==prefix&&/^runs\/contact-planning\/[a-z0-9-]+$/.test(prefix));
const read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const scene=read(source+'.scene.json'),config=read(source+'.config.json');
const sharedPolicy=fs.readFileSync(d+'/contact-planned.rhai','utf8');
assert(scene.controller.parameters.point_feedback_gain===undefined&&
 (!scene.controller.sources.files['contact-planned.rhai'].includes('point_correction')||
  scene.controller.sources.files['contact-planned.rhai']===sharedPolicy),
 'source must have no point feedback or the shared inactive optional feedback branch');
const multiplier=Number(process.argv[4]??1);
assert(Number.isFinite(multiplier)&&multiplier>0,'positive explicit damping multiplier required');
const markers=config.policy.task_observations.markers,motors=config.motors.effective.components;
const times=motors.map(m=>m.parameters.damping/m.parameters.stiffness);
assert(times.every(t=>Number.isFinite(t)&&t>0&&Math.abs(t-times[0])<1e-12),'common declared effective-servo time constant required');
assert(scene.robot.gravity[0]===0&&scene.robot.gravity[1]===0&&scene.robot.gravity[2]<0);
const mass=scene.robot.links.filter(l=>!l.ground).reduce((a,l)=>a+l.mass,0),weight=-scene.robot.gravity[2]*mass;
config.policy.feedback_observations=true;
config.policy.body_feedback=null;
config.policy.point_feedback={expected_cad_sha256:scene.robot.source.cad_sha256,
 coordinate_frame:'r1357-export-world-Z-up-m',markers,
 position_world_m:{keyframes:[{time_s:0,values:markers.flatMap(()=>[0,0,0])}]},
 activation:{keyframes:[{time_s:0,values:markers.map(()=>1)}]},position_gain:0,
 damping_m_per_rad:.001,maximum_correction_rad:.05,
 floor_velocity_damping:{velocity_damping_s:times[0]*multiplier,full_support_force_n:weight/markers.length}};
scene.controller.parameters.point_feedback_gain=1;
scene.controller.sources.files['contact-planned.rhai']=sharedPolicy;
for(const [suffix,value] of [['scene',scene],['config',config],['actions',read(source+'.actions.json')]])fs.writeFileSync(prefix+'.'+suffix+'.json',JSON.stringify(value)+'\n',{flag:'wx'});
const report={source,prefix,inputs:['scene','config','actions'].map(k=>({path:source+'.'+k+'.json',sha256:sha(source+'.'+k+'.json')})),
 source_policy_sha256:sha(d+'/contact-planned.rhai'),mass_kg:mass,weight_n:weight,velocity_damping_s:times[0]*multiplier,
 damping_multiplier:multiplier,
 windows_s:[[.8,3],[4.2,6.4]],
 command_speed_m_s:Math.max(...read(source+'.actions.json').map(a=>Math.abs(a[scene.controller.inputs.findIndex(i=>i.name==='command.forward_speed')]))),
 full_support_force_n:weight/markers.length,position_gain:0,maximum_correction_rad:.05,damping_m_per_rad:.001,
 scope:'Replaces the inherited unused point-feedback observation recipe, which the source policy did not consume. Diagnostic teacher feedback from privileged actual loaded contact material velocity. Time constant is effective servo damping/stiffness times the explicit controller multiplier; full-support load is CAD model weight/foot count. 0.001 m/rad inverse-Jacobian damping and 0.05 rad command cap are explicit numerical/controller choices, not robot properties. No position-path attraction, pose mutation or additional physical damping; policy changes ordinary servo targets. Contact model, actuators, geometry, actions and timestep are unchanged. Hardware sensing and transfer remain uncalibrated.'};
fs.writeFileSync(d+'/'+prefix.split('/').at(-1)+'-trial.json',JSON.stringify(report,null,2)+'\n',{flag:'wx'});console.log(report);
