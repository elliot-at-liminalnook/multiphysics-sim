import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/auxiliary-condensation';
const browser='runs/interactive/auxiliary-condensation';
const artifacts={};
async function bytes(path) {
  const b=await readFile(path);artifacts[path]=createHash('sha256').update(b).digest('hex');return b;
}
async function read(path) {return JSON.parse(await bytes(path));}
const manifest=await read(directory+'/manifest.json');
const sumWork=d=>{
  const sums={};
  for(const s of d.hybrid_solves)for(const [key,value] of Object.entries(s)){
    if(key.startsWith('successful_'))sums[key]=(sums[key]??0)+value;
  }
  return sums;
};
const cases=[];
for(const name of ['base','refined']) {
  const d=await read(`${directory}/${name}.execution.json`);
  const r=await read(manifest.comparisons[name]);
  const c=await read(`${directory}/${name}.comparison.json`);
  const lift=await read(`${directory}/${name}.lift.json`);
  await bytes(`${directory}/${name}.config.json`);
  assert(d.completed&&d.error===null&&r.completed&&r.error===null);
  assert.equal(d.implicit.condense_auxiliary,true);
  assert.equal(d.scene_options.motor_dynamics??'detailed','detailed');
  const maxima=c.maximum_sampled_differences;
  const marker=Math.max(...c.markers.map(m=>m.maximum_error_m));
  const impulse=Math.max(...c.contact_impulse_comparison.total.map(m=>m.difference_norm_ns));
  const eventCountsMatch=c.events.every(e=>e.candidate_count===e.reference_count);
  const eventTimes=c.events.map(e=>e.maximum_ordered_time_difference_s).filter(x=>x!==null);
  const event=Math.max(0,...eventTimes);
  const closure={};
  for(const f of d.frames)for(const row of f.original_rows){
    const scale=row.unit==='m'?d.embedding.length_scale_m:row.unit==='rad'?d.embedding.angle_scale_rad:1;
    const maximum=closure[row.unit]??={position:0,velocity:0,acceleration:0};
    for(const key of Object.keys(maximum)){
      assert(Number.isFinite(row[key]));
      maximum[key]=Math.max(maximum[key],Math.abs(row[key]));
      assert(Math.abs(row[key])/scale<=d.embedding.scaled_closure_tolerance);
    }
  }
  cases.push({name,step_s:d.step_s,simulated_s:d.simulated_s,
    candidate_wall_s:d.stepping_wall_s,reference_wall_s:r.stepping_wall_s,
    candidate_successful_trial_work:sumWork(d),reference_successful_trial_work:sumWork(r),
    maximum_marker_difference_m:marker,maximum_contact_impulse_difference_ns:impulse,
    maximum_event_time_difference_s:event,event_counts_match:eventCountsMatch,
    sampled_contact_pair_mismatches:c.contact_pair_mismatch_samples,
    maximum_sampled_differences:maxima,
    strict_numerical_equivalence_passed:marker<1e-9&&impulse<1e-7&&event<1e-8&&eventCountsMatch&&
      maxima.joint_positions<1e-8&&maxima.joint_velocities<1e-6&&maxima.current_a<1e-7&&
      maxima.motor_voltage_v<1e-6&&maxima.shaft_torque_nm<1e-7&&c.contact_pair_mismatch_samples===0,
    original_closure_maxima:closure,lift:lift.report,
    maximum_auxiliary_residual:Math.max(...d.hybrid_solves.map(s=>s.maximum_auxiliary_residual)),
    accepted_segments:d.hybrid_steps.reduce((n,s)=>n+s.accepted_segments,0),
    rejected_trials:d.hybrid_steps.reduce((n,s)=>n+s.rejected_trials,0),
    rejection_details:d.hybrid_steps.flatMap((s,i)=>s.rejection_details.map(e=>({nominal_step:i,...e}))),
    profile:d.solver_profile,
  });
}
const divergence=await read(directory+'/base.divergence.json');
const refinedDivergence=await read(directory+'/refined.divergence.json');
const diagnostic=await read(directory+'/diagnostic.divergence.json');
for(const mode of ['reference','condensed']) {
  await bytes(`${directory}/diagnostic.${mode}.config.json`);
  const prefix=await read(`${directory}/diagnostic.${mode}.execution.json`);
  assert.equal(prefix.completed_steps,120);
  assert.equal(prefix.requested_steps,120);
  assert.equal(prefix.completed,false);
  assert(prefix.error.startsWith('simulation horizon ended before motion completed:'));
}
const portable=await read(browser+'/pendulum-browser.json'),ui=await read(browser+'/viewer-report.json');
assert(portable.passed&&ui.passed);
for(const path of [
  'crates/sim-domain-robot/src/articulated/embedding/implicit.rs',
  'crates/sim-domain-robot/src/articulated/embedding/motor_step.rs',
  'crates/sim-domain-robot/tests/embedded_motor.rs','crates/sim-domain-robot/tests/embedded_step.rs',
  'crates/sim-runtime/tests/embedded_session.rs','examples/interactive/pendulum.condensed.json',
  'examples/full-robot/prepare_auxiliary_condensation.mjs','examples/full-robot/summarize_auxiliary_condensation.mjs',
  'examples/full-robot/auxiliary-condensation-validation.md','web/tests/viewer.mjs','.github/workflows/browser.yml',
  browser+'/native-runner',browser+'/source-manifest.json',browser+'/pendulum.execution.json',
  browser+'/viewer/build-manifest.json',browser+'/viewer/sim_web_bg.wasm',
  'runs/interactive/robot-lab-auxiliary-condensation-2026-09-07.zip',
]) await bytes(path);
await writeFile('examples/full-robot/auxiliary-condensation-status.json',JSON.stringify({
  milestone:'generic_auxiliary_condensation',promoted_default:false,training_model_accepted:false,
  scope:'Detailed equations, trial-local nonlinear auxiliary elimination, no assumed motor independence. Numerical equivalence, sampled task outcome and performance are separate. Timings include concurrent development work; no controlled speedup claim. Small browser fixture portability does not prove full-robot portability or hardware accuracy.',
  manifest,cases,divergence,refined_divergence:refinedDivergence,diagnostic,browser:portable,ui,artifacts,
},null,2));
console.log(JSON.stringify(cases.map(c=>({case:c.name,wall_s:c.candidate_wall_s,maximum_foot_m:c.maximum_marker_difference_m,current_a:c.maximum_sampled_differences.current_a,strict_pass:c.strict_numerical_equivalence_passed,lift:c.lift.passed}))));
