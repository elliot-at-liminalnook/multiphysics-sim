// Experiment assembly only; shared Rust owns controller helpers and physics.
import fs from 'node:fs';
import crypto from 'node:crypto';
const dir='examples/full-robot/contact-planning/';
const base='runs/speed-ceiling/validation/constrained-front165-scale1-human-fine';
const name=process.argv[3]??'compiled184-screen';
if(!/^[a-z0-9-]+$/.test(name))throw Error('named trial required');
const prefix='runs/contact-planning/'+name;
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const referencePath=process.argv[2]??dir+'controller-reference.result.json';
const reference=read(referencePath);
const scene=read(base+'.scene.json'),config=read(base+'.config.json');
if(scene.robot.source.cad_sha256!==reference.recipe.expected_cad_sha256)
  throw Error('CAD mismatch');
const old=scene.controller.parameters;
const names=reference.recipe.independent_coordinates;
const hips=names.flatMap((name,i)=>name.endsWith('Hip servo output')?[i]:[]);
if(hips.length!==reference.motion.feet.length || hips.length!==old.yaw_jacobian_ratios.length)
  throw Error('explicit hip/foot steering bindings required');
scene.controller.sources={entry:'contact-planned.rhai',files:{
  'contact-planned.rhai':fs.readFileSync(dir+'contact-planned.rhai','utf8')}};
scene.controller.parameters={
  reference_load_audit:{diagnostic_allow_failed_reference_audit:reference.diagnostic_allow_failed_reference_audit??false,
    required_load_audits_passed:reference.required_load_audits_passed??null,
    nominal:reference.nominal_physical_summary,reverse:reference.reverse_load_audit??null},
  motion:reference.motion,period_s:reference.motion.period_s,
  nominal_speed_m_s:reference.nominal_speed_m_s,trajectory:reference.trajectory,
  static_feedforward:reference.static_feedforward,dynamic_feedforward:reference.dynamic_feedforward,
  phase_offset_s:reference.phase_offset_s,pause_windows_s:reference.pause_windows_s,
  initial_phase_s:reference.initial_phase_s,motor_indices:old.motor_indices,
  hip_motor_indices:hips,yaw_jacobian_ratios:old.yaw_jacobian_ratios,
  maximum_yaw_offset_rad:0.05,maximum_feedforward_offset_rad:0.15,
  velocity_lead_s:names.map(name=>reference.recipe.actuators[name].damping/reference.recipe.actuators[name].stiffness),
  command_lease:old.command_lease,acceleration_m_s2:0.4,deceleration_m_s2:0.8,reversal_settle_s:0.08,
  travel_heading_offset_rad:Math.atan2(reference.motion.displacement_world_m[1],reference.motion.displacement_world_m[0]),
};
if(reference.velocity_feedforward)scene.controller.parameters.velocity_feedforward=reference.velocity_feedforward;
for(const input of scene.controller.inputs) if(input.name==='command.forward_speed') {
  input.lower=-reference.nominal_speed_m_s;input.upper=reference.nominal_speed_m_s;input.initial=0;
}
scene.duration_s=8;
config.initial_coordinates=reference.initial_coordinates;
config.initial_base_translation_m=reference.initial_base_translation_m;
config.initial_base_rotation_vector_rad=reference.initial_base_rotation_vector_rad;
config.motors.servos.forEach((s,i)=>{s.target_rad=reference.initial_coordinates[i];});
config.motors.target_trajectory=null;
config.step_s=0.000625;config.steps=Math.round(scene.duration_s/config.step_s);
config.report_every=Math.round(scene.period_s/config.step_s);
const actions=Array.from({length:Math.round(scene.duration_s/scene.period_s)},(_,i)=>{
  const t=i*scene.period_s;
  const speed=t>=0.6&&t<3?reference.nominal_speed_m_s:t>=4&&t<6.4?-reference.nominal_speed_m_s:0;
  return scene.controller.inputs.map(ch=>ch.name==='command.forward_speed'?speed:
    ch.name==='command.packet_sequence'?i+1:ch.initial);
});
fs.mkdirSync('runs/contact-planning',{recursive:true});
for(const [suffix,value] of [['scene',scene],['config',config],['actions',actions]])
  fs.writeFileSync(prefix+'.'+suffix+'.json',JSON.stringify(value)+'\n',{flag:'wx'});
fs.writeFileSync(dir+(process.argv[3]?name+'-trial.json':'controller-trial.json'),JSON.stringify({
  prefix,source_scene:base+'.scene.json',source_scene_sha256:hash(base+'.scene.json'),
  compiled_reference:referencePath,compiled_reference_sha256:hash(referencePath),
  travel_heading_offset_rad:scene.controller.parameters.travel_heading_offset_rad,
  reference_load_audit:scene.controller.parameters.reference_load_audit,
  policy:dir+'contact-planned.rhai',policy_sha256:hash(dir+'contact-planned.rhai'),
  files:['scene','config','actions'].map(kind=>({path:prefix+'.'+kind+'.json',sha256:hash(prefix+'.'+kind+'.json')})),
  task:'examples/full-robot/fast-wasd/task.json',duration_s:scene.duration_s,step_s:config.step_s,
  scope:'First closed-loop forward/reverse/stop screen of the automatically planned contact motion. Source CAD, contact physics, effective actuators, software bounds and lease remain unchanged. Initial pose and reference are explicit. Static plus odd/even clock-rate feedforward is an approximation during transitions; inherited local yaw ratios still require separate steering validation. User browser bundles are untouched. No success or hardware-transfer claim before measurement.'
},null,2)+'\n',{flag:'wx'});
