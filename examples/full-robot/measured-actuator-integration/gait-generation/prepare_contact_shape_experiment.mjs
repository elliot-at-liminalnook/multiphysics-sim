// Bind a shared-Rust contact-reference compilation to the detailed motor experiment.
// No kinematics, force computation, or simulation is implemented here.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
const [parentPath,compiledPath,out,seconds='5']=process.argv.slice(2);
if(!out)throw Error('prepare_contact_shape_experiment.mjs PARENT_SPEC COMPILED_REFERENCE NEW_DIRECTORY [seconds]');
const read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const spec=read(parentPath),c=read(compiledPath),p=spec.scene.controller.parameters;
assert.equal(c.recipe.expected_cad_sha256,spec.scene.robot.source.cad_sha256);
assert.deepEqual(c.recipe.independent_coordinates,spec.config.motors.target_coordinates);
assert(c.pause_windows_s.length>0&&c.nominal_speed_m_s>0);
const physical=JSON.stringify(spec.scene.robot);
for(const key of ['motion','trajectory','static_feedforward','dynamic_feedforward','velocity_feedforward','phase_offset_s','pause_windows_s','initial_phase_s','nominal_speed_m_s'])p[key]=c[key];
p.period_s=c.motion.period_s;p.motion_amplitude=1;p.velocity_lead_scale=0;
p.reference_centers=p.trajectory.keyframes[0].values.map((_,i)=>p.trajectory.keyframes.reduce((sum,k)=>sum+k.values[i],0)/p.trajectory.keyframes.length);
p.reference_load_audit={scope:'Proposal diagnostic only; actual detailed motors/contact decide walking performance.',required_load_audits_passed:c.required_load_audits_passed,nominal:c.nominal_physical_summary,reverse:c.reverse_load_audit,maximum_reference_errors:c.maximum_reference_errors};
// The Rhai program does not use body/point feedback. Avoid stale references from
// the old gait; physical task observations remain recorded independently.
assert(!('point_feedback_gain' in p));
spec.config.policy.body_feedback=null;spec.config.policy.point_feedback=null;
spec.config.initial_coordinates=c.initial_coordinates;
spec.config.initial_base_translation_m=c.initial_base_translation_m;
spec.config.initial_base_rotation_vector_rad=c.initial_base_rotation_vector_rad;
spec.config.motors.servos.forEach((servo,i)=>servo.target_rad=c.initial_coordinates[i]);
const values={pace_scale:1,motion_amplitude:1,tracking_gain:0,velocity_lead_scale:0};
spec.baseline=values;
for(const parameter of spec.parameterization.space.parameters)parameter.bounds=[values[parameter.name],values[parameter.name]];
for(const binding of spec.parameterization.scalars)binding.reference=p[binding.pointer.slice(1)];
const duration=Number(seconds),dt=spec.scene.period_s;
assert(duration>=3&&duration<=30&&Number.isInteger(duration/dt));
spec.config.steps=Math.round(duration/spec.config.step_s);spec.scene.duration_s=duration;
const inputs=spec.scene.controller.inputs;
const forward=inputs.findIndex(i=>i.name==='command.forward_speed'),yaw=inputs.findIndex(i=>i.name==='command.yaw_rate'),sequence=inputs.findIndex(i=>i.name==='command.packet_sequence');
assert(forward>=0&&yaw>=0&&sequence>=0);
assert(c.nominal_speed_m_s<=inputs[forward].upper);
spec.source_actions=Array.from({length:duration/dt},(_,i)=>{const row=inputs.map(x=>x.initial);row[forward]=c.nominal_speed_m_s;row[yaw]=0;row[sequence]=i+1;return row;});
assert.equal(JSON.stringify(spec.scene.robot),physical,'Physical robot definition changed');
fs.mkdirSync(out);
fs.writeFileSync(out+'/spec.json',JSON.stringify(spec)+'\n',{flag:'wx'});
fs.writeFileSync(out+'/preparation.json',JSON.stringify({parent:{path:parentPath,sha256:sha(parentPath)},compiled:{path:compiledPath,sha256:sha(compiledPath)},script_sha256:sha(new URL(import.meta.url)),duration_s:duration,seed:spec.seed,cad_sha256:spec.scene.robot.source.cad_sha256,physical_definition_unchanged:true,initial_pose:'Explicit candidate-specific closed at-rest stance from shared Rust IK; source geometry, mass, mechanics, actuator profiles and world unchanged.',policy:'Compiled contact timing/path/stance, no old-gait amplitude or velocity lead; same bounded reference and integer motor controller.',commands:'Constant nominal forward request with fresh packet sequence; zero yaw. Actual travel is measured from full simulation.',comparison:'Prior historical schedule included yaw; matched straight-line baseline is required before speed promotion.',required_load_audits_passed:c.required_load_audits_passed},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({out,duration_s:duration,nominal_speed_m_s:c.nominal_speed_m_s,period_s:p.period_s}));
