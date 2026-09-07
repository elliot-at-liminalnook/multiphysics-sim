import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const dir='runs/full-robot/learning/fast-motor',artifacts={};
async function bytes(path){const b=await readFile(path);artifacts[path]=createHash('sha256').update(b).digest('hex');return b;}
async function read(path){return JSON.parse(await bytes(path));}
const manifest=await read(dir+'/manifest.json'),limits=manifest.preliminary_screen;
const fine=await read('runs/full-robot/learning/sample-reuse/final-refresh.full.execution.json');
const detailed=await read(dir+'/detailed.execution.json');
const cases=[];
for(const [captureFile,comparisonFile,reference,liftFile] of [
 ['detailed','detailed.refinement',fine,'detailed'],
 ['quasistatic','quasistatic',fine,'quasistatic'],
 ['quasistatic.refined','quasistatic.refined',fine,'quasistatic.refined'],
 ['quasistatic','quasistatic.same-step',detailed,'quasistatic'],
 ['quasistatic_winding','winding.same-step',detailed,'quasistatic_winding'],
 ['quasistatic_rotor','rotor.same-step',detailed,'quasistatic_rotor']
]) {
 const capture=await read(`${dir}/${captureFile}.execution.json`);
 const comparison=await read(`${dir}/${comparisonFile}.comparison.json`);
 const lift=await read(`${dir}/${liftFile}.lift.json`);
 assert(capture.completed && reference.completed && !capture.error && !reference.error);
 assert(isDeepStrictEqual(capture.independent_joint_indices,reference.independent_joint_indices));
 assert.equal(capture.frames.length,reference.frames.length);
 let angle=0;
 for(let i=0;i<capture.frames.length;i++) {
  assert(Math.abs(capture.frames[i].time_s-reference.frames[i].time_s)<1e-12);
  for(const joint of capture.independent_joint_indices) {
   const error=Math.abs(capture.frames[i].joint_positions[joint]-reference.frames[i].joint_positions[joint]);
   assert(Number.isFinite(error));angle=Math.max(angle,error);
  }
 }
 const foot=Math.max(...comparison.markers.map(m=>m.maximum_error_m));
 const rms=Math.max(...comparison.markers.map(m=>m.rms_error_m));
 const impulses=comparison.contact_impulse_comparison.total.map(i=>({link:i.link,other:i.other,
  error_ns:i.difference_norm_ns,limit_ns:Math.max(limits.absolute_impulse_floor_ns,
   limits.maximum_relative_per_foot_impulse_difference*Math.hypot(...i.reference_ns))}));
 const checks={foot:foot<=limits.maximum_foot_difference_m,rms:rms<=limits.rms_foot_difference_m,
  motor_angle:angle<=limits.maximum_motor_angle_difference_rad,
  impulses:impulses.every(i=>i.error_ns<=i.limit_ns),supported_lift:lift.report.passed};
 cases.push({capture:captureFile,comparison:comparisonFile,mode:capture.scene_options.motor_dynamics??'detailed',
  step_s:capture.step_s,reference_step_s:reference.step_s,simulated_s:capture.simulated_s,wall_s:capture.stepping_wall_s,
  maximum_foot_difference_m:foot,maximum_foot_rms_m:rms,maximum_motor_angle_difference_rad:angle,
  impulses,checks,preliminary_screen_passed:Object.values(checks).every(Boolean),
  maximum_sampled_differences:comparison.maximum_sampled_differences,lift:lift.report,
  event_count_mismatches:comparison.events.filter(e=>e.candidate_count!==e.reference_count).length,
  sampled_contact_pair_mismatches:comparison.contact_pair_mismatch_samples,
  rejected_trials:capture.hybrid_steps.reduce((n,s)=>n+s.rejected_trials,0),profile:capture.solver_profile});
}
for(const file of ['config.json','refined.config.json','quasistatic.scene.json','quasistatic_winding.scene.json','quasistatic_rotor.scene.json'])await bytes(dir+'/'+file);
for(const file of ['examples/full-robot/prepare_fast_motor.mjs','examples/full-robot/summarize_fast_motor.mjs',
 'examples/full-robot/fast-motor-validation.md','crates/sim-domain-robot/src/motor.rs',
 'crates/sim-domain-robot/tests/motor_reduction.rs','crates/sim-domain-robot/tests/motor_jacobian.rs',
 'crates/sim-runtime/src/physical.rs','crates/sim-runtime/src/embedded.rs','crates/sim-runtime/tests/embedded_session.rs',
 'crates/sim-runtime/examples/compare_embedding.rs','runs/interactive/fast-motor/native-runner',
 'runs/interactive/fast-motor/source-manifest.json'])await bytes(file);
await writeFile('examples/full-robot/fast-motor-status.json',JSON.stringify({
 milestone:'explicit_motor_dynamics_reduction_screen',promoted:false,training_model_accepted:false,
 scope:'Physical reduction and timestep experiments. Same-step comparisons isolate storage-term approximations; fine-reference comparisons include timestep effects. Preliminary task screens are not calibrated hardware acceptance. Sampled current/heat differences do not resolve switching spikes or establish electrical energy/thermal accuracy. Timings include concurrent development work, not controlled repeated benchmarks.',
 limits,cases,artifacts
},null,2));
console.log(cases.map(c=>({comparison:c.comparison,screen:c.preliminary_screen_passed,foot_mm:c.maximum_foot_difference_m*1000,motor_rad:c.maximum_motor_angle_difference_rad})));
