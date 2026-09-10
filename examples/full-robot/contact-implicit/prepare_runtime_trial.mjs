// Configuration assembly only; Rust compiles references and advances physics.
import fs from 'node:fs';import crypto from 'node:crypto';import assert from 'node:assert/strict';
const [referencePath,name,subdivisionsText,gainText,policySamplesText]=process.argv.slice(2);
assert(referencePath && /^[a-z0-9-]+$/.test(name));
const subdivisions=Number(subdivisionsText),gain=Number(gainText),policySamples=Number(policySamplesText??8);
assert([32,64,128].includes(subdivisions) && [0,1].includes(gain));
assert([4,8].includes(policySamples) && subdivisions%policySamples===0);
const read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const source='runs/speed-ceiling/validation/constrained-front165-scale1-human-fine';
const reference=read(referencePath),scene=read(source+'.scene.json'),config=read(source+'.config.json');
const task=read('examples/full-robot/fast-wasd/task.json');
assert.equal(scene.robot.source.cad_sha256,reference.expected_cad_sha256);
assert.deepEqual(config.motors.target_coordinates,reference.independent_coordinates);
assert(reference.planning.within_planning_tolerances);
const indices=scene.controller.parameters.motor_indices;
assert.equal(Object.keys(indices).length,reference.independent_coordinates.length);
const policy='examples/full-robot/contact-implicit/finite-plan.rhai';
scene.controller.sources={entry:'finite-plan.rhai',files:{'finite-plan.rhai':fs.readFileSync(policy,'utf8')}};
scene.controller.parameters={trajectory:reference.trajectory,step_s:reference.step_s,
  velocity_offsets:reference.velocity_offsets,torque_offsets:reference.torque_offsets,
  feedforward_gain:gain,motor_indices:indices};
const intervals=reference.trajectory.keyframes.length-1;
scene.duration_s=reference.duration_s;scene.period_s=reference.step_s/policySamples;
config.initial_coordinates=reference.initial_coordinates;
config.initial_base_translation_m=reference.initial_base_translation_m;
config.initial_base_rotation_vector_rad=reference.initial_base_rotation_vector_rad;
config.motors.servos.forEach((servo,j)=>{servo.target_rad=reference.initial_coordinates[j];});
config.motors.target_trajectory=null;
config.step_s=reference.step_s/subdivisions;config.steps=intervals*subdivisions;
// Effective-servo integration currently records contact at report endpoints;
// its separate per-step contact audit is unsupported (scheduled motors only).
config.report_every=subdivisions/policySamples;config.audit_contact_steps=false;
// Session construction and embedded integration must declare compatible clocks.
scene.options.step=config.step_s;scene.options.sample=scene.period_s;
task.period_s=scene.period_s;
const actions=Array.from({length:intervals*policySamples},(_,i)=>scene.controller.inputs.map(ch=>
  ch.name==='command.packet_sequence'?i+1:ch.initial));
const prefix='runs/contact-implicit/'+name;
fs.mkdirSync('runs/contact-implicit',{recursive:true});
const files=[['scene',scene],['config',config],['task',task],['actions',actions]].map(([kind,value])=>{
  const path=prefix+'.'+kind+'.json';fs.writeFileSync(path,JSON.stringify(value)+'\n',{flag:'wx'});return{path,sha256:sha(path)};
});
fs.writeFileSync('examples/full-robot/contact-implicit/'+name+'.trial.json',JSON.stringify({
  prefix,files,source_scene:{path:source+'.scene.json',sha256:sha(source+'.scene.json')},
  source_config:{path:source+'.config.json',sha256:sha(source+'.config.json')},
  compiled_reference:{path:referencePath,sha256:sha(referencePath)},policy:{path:policy,sha256:sha(policy)},
  step_s:config.step_s,policy_samples_per_plan_interval:policySamples,controller_period_s:scene.period_s,duration_s:scene.duration_s,feedforward_gain:gain,
  scope:'Finite-horizon timed diagnostic through the normal Rust/Rhai environment and effective servo envelopes. Source contact profile retained: sampled CAD surfaces with regularized Coulomb friction (1 mm/s), interlink force response omitted and checked separately by geometry audit. Session construction clocks align with embedded/controller steps. Existing task tilt/height bounds retained. No command-response, endpoint hold, periodic or sustained gait claim; browser bundles unchanged.'
},null,2)+'\n',{flag:'wx'});
console.log({prefix,steps:config.steps,step_s:config.step_s,controller_period_s:scene.period_s});
