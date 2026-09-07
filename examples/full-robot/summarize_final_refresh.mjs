import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/sample-reuse',browser='runs/interactive/final-refresh';
const artifacts={};
async function bytes(path) {
  const b=await readFile(path);artifacts[path]=createHash('sha256').update(b).digest('hex');return b;
}
async function read(path) { return JSON.parse(await bytes(path)); }
const manifest=await read(directory+'/final-refresh.manifest.json');
const cases=[];
for(const name of ['base','full']) {
  const capture=await read(`${directory}/final-refresh.${name}.execution.json`);
  const comparison=await read(`${directory}/final-refresh.${name}.comparison.json`);
  const lift=await read(`${directory}/final-refresh.${name}.lift.json`);
  assert(capture.completed && capture.error===null);
  const state=comparison.maximum_sampled_differences;
  const marker=Math.max(...comparison.markers.map(x=>x.maximum_error_m));
  const impulse=Math.max(...comparison.contact_impulse_comparison.total.map(x=>x.difference_norm_ns));
  const event=Math.max(...comparison.events.map(x=>x.maximum_ordered_time_difference_s));
  const eventCountsMatch=comparison.events.every(x=>x.candidate_count===x.reference_count);
  const closure={};
  for(const frame of capture.frames)for(const row of frame.original_rows) {
    const scale=row.unit==='m'?capture.embedding.length_scale_m:row.unit==='rad'?capture.embedding.angle_scale_rad:1;
    const maximum=closure[row.unit]??={position:0,velocity:0,acceleration:0};
    for(const key of Object.keys(maximum)) {
      maximum[key]=Math.max(maximum[key],Math.abs(row[key]));
      assert(Math.abs(row[key])/scale<=capture.embedding.scaled_closure_tolerance);
    }
  }
  cases.push({name,step_s:capture.step_s,simulated_s:capture.simulated_s,wall_s:capture.stepping_wall_s,
    maximum_marker_difference_m:marker,maximum_contact_impulse_difference_ns:impulse,
    maximum_event_time_difference_s:event,maximum_sampled_differences:state,event_counts_match:eventCountsMatch,
    sampled_contact_pair_mismatches:comparison.contact_pair_mismatch_samples,
    strict_roundoff_gate_passed:marker<1e-9&&impulse<1e-7&&event<1e-8&&
      state.joint_positions<1e-8&&state.joint_velocities<1e-6&&state.current_a<1e-7&&
      state.motor_voltage_v<1e-6&&state.shaft_torque_nm<1e-7&&eventCountsMatch&&comparison.contact_pair_mismatch_samples===0,
    original_closure_maxima:closure,lift:lift.report,
    accepted_segments:capture.hybrid_steps.reduce((n,s)=>n+s.accepted_segments,0),
    rejected_trials:capture.hybrid_steps.reduce((n,s)=>n+s.rejected_trials,0),
    successful_trial_endpoint_evaluations:capture.hybrid_solves.reduce((n,s)=>n+s.successful_trial_endpoint_evaluations,0),
    profile:capture.solver_profile});
}
const browserResult=await read(browser+'/robot-browser.json'),ui=await read(browser+'/viewer-report.json');
const focused=await read(directory+'/final-refresh-divergence.json');
for(const file of ['config.json','base.config.json','full.config.json','viewer.config.json','execution.json'])await bytes(directory+'/final-refresh.'+file);
for(const file of ['newton-audit.config.json','newton-audit.execution.json','diagnostic.reference.execution.json',
  'reference.execution.json','refined.reference.execution.json'])await bytes(directory+'/'+file);
for(const file of ['examples/full-robot/prepare_final_refresh.mjs','examples/full-robot/summarize_final_refresh.mjs',
  'examples/full-robot/newton-convergence-validation.md','crates/sim-solve/src/lib.rs','crates/sim-solve/tests/convergence.rs',
  'crates/sim-domain-robot/src/articulated/embedding/implicit.rs','crates/sim-domain-robot/tests/embedded_motor.rs',
  'runs/interactive/sample-reuse/final-refresh-native-runner',browser+'/source-manifest.json',
  'runs/interactive/robot-lab-final-refresh-2026-09-07.zip',
  browser+'/viewer/build-manifest.json',browser+'/viewer/sim_web_bg.wasm'])await bytes(file);
await writeFile('examples/full-robot/final-refresh-status.json',JSON.stringify({
  milestone:'fresh_final_newton_corrections',promoted_default:false,task_accepted:false,
  scope:'Opt-in final-iteration matrix refresh with unchanged tolerances and cap. Focused horizon diagnostics and complete-motion gates are separate. Native timings include concurrent development work; not an isolated speed comparison. Passing numerical gates would not establish calibrated hardware behavior, walking, or realtime.',
  manifest,focused,cases,browser:browserResult,ui,artifacts
},null,2));
console.log('Saved full-trajectory, task, browser and provenance evidence for final-iteration refresh.');
