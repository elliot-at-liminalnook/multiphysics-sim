import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const dir='runs/full-robot/learning/sample-reuse',browser='runs/interactive/sample-reuse',artifacts={};
async function bytes(p){const b=await readFile(p);artifacts[p]=createHash('sha256').update(b).digest('hex');return b;}
async function read(p){return JSON.parse(await bytes(p));}
const manifest=await read(dir+'/manifest.json');
const cases=[];
for(const prefix of ['','refined.']){
 const capture=await read(dir+'/'+prefix+'execution.json'),comparison=await read(dir+'/'+(prefix?'refined-paired-comparison':'paired-comparison')+'.json'),lift=await read(dir+'/'+prefix+'lift.json');
 assert(capture.completed&&capture.error===null);assert(lift.report.passed);
 const marker=Math.max(...comparison.markers.map(m=>m.maximum_error_m));
 const impulse=Math.max(...comparison.contact_impulse_comparison.total.map(c=>c.difference_norm_ns));
 const event=Math.max(...comparison.events.map(e=>e.maximum_ordered_time_difference_s));
 const strictRoundoffGate = marker<1e-9&&impulse<1e-7&&event<1e-8;
 assert(comparison.events.every(e=>e.candidate_count===e.reference_count));
 assert.equal(comparison.contact_pair_mismatch_samples,0);
 const d=comparison.maximum_sampled_differences;
 const stateGate=d.joint_positions<1e-8&&d.joint_velocities<1e-6&&d.current_a<1e-7&&d.motor_voltage_v<1e-6&&d.shaft_torque_nm<1e-7;
 const closure={};
 for(const frame of capture.frames)for(const row of frame.original_rows){
  const maxima=closure[row.unit]??={position:0,velocity:0,acceleration:0};
  const scale=row.unit==='m'?capture.embedding.length_scale_m:row.unit==='rad'?capture.embedding.angle_scale_rad:1;
  for(const key of Object.keys(maxima)){maxima[key]=Math.max(maxima[key],Math.abs(row[key]));assert(Math.abs(row[key])/scale<=capture.embedding.scaled_closure_tolerance);}
 }
 cases.push({implicit:capture.implicit,successful_trial_endpoint_evaluations:capture.hybrid_solves.reduce((n,s)=>n+s.successful_trial_endpoint_evaluations,0),successful_trials_with_reused_jacobian:capture.hybrid_solves.reduce((n,s)=>n+s.successful_trials_with_reused_jacobian,0),strict_roundoff_gate_passed:strictRoundoffGate&&stateGate,step_s:capture.step_s,simulated_s:capture.simulated_s,wall_s:capture.stepping_wall_s,lift:lift.report,maximum_marker_difference_m:marker,maximum_contact_impulse_difference_ns:impulse,maximum_event_time_difference_s:event,maximum_sampled_differences:d,original_closure_maxima:closure,profile:capture.solver_profile});
}
const reference=await read(dir+'/reference.execution.json');
const refinedReference=await read(dir+'/refined.reference.execution.json');
await read(dir+'/refined-comparison.json');
await read(dir+'/refined-divergence.json');
const diagnosis=await read(dir+'/diagnostic-divergence.json');
const diagnosticPrefix=await read(dir+'/diagnostic-prefix-check.json');
assert(diagnosticPrefix.cases.every(c=>c.frames_exact&&c.scheduler_exact));
for(const name of ['diagnostic.execution.json','diagnostic.reference.execution.json'])await bytes(dir+'/'+name);
await read('runs/full-robot/learning/point-feedback/refined.execution.json');
const browserResult=await read(browser+'/robot-browser.json'),ui=await read(browser+'/viewer-report.json');assert(ui.passed&&browserResult.replay_exact&&browserResult.reset_exact);
for(const p of ['config.json','viewer.config.json','reference.config.json','refined.config.json','refined.reference.config.json','diagnostic.config.json','diagnostic.reference.config.json','refinement.json'])await read(dir+'/'+p);
for(const p of ['examples/full-robot/sample-reuse-experiment.json','examples/full-robot/sample-reuse-validation.md','examples/full-robot/prepare_sample_reuse.mjs','examples/full-robot/summarize_sample_reuse.mjs','crates/sim-domain-robot/src/articulated/embedding.rs','crates/sim-domain-robot/src/articulated/embedding/implicit.rs','crates/sim-domain-robot/src/articulated/embedding/control.rs','crates/sim-domain-robot/src/articulated/embedding/servos.rs','crates/sim-domain-robot/src/articulated/embedding/motor_step.rs','crates/sim-domain-robot/tests/embedded_motor.rs','crates/sim-dynamics/src/hybrid.rs','crates/sim-dynamics/tests/hybrid_stepper.rs','examples/interactive/audit_hybrid_divergence.mjs','crates/sim-runtime/examples/compare_embedding.rs',browser+'/native-runner',browser+'/diagnostic-native-runner',browser+'/source-manifest.json',browser+'/viewer/build-manifest.json',browser+'/viewer/sim_web_bg.wasm'])await bytes(p);
await writeFile('examples/full-robot/sample-reuse-status.json',JSON.stringify({milestone:'guarded_controller_sample_jacobian_reuse',strict_roundoff_gate_passed:cases.every(c=>c.strict_roundoff_gate_passed),browser_parity_passed:browserResult.passed,promoted_default:false,task_accepted:false,
 scope:'Opt-in numerical matrix reuse across eligible controller samples. Guarded controller-sample reuse experiment; explicit strict numerical gates, task measurements and browser results are recorded separately. Existing controller/model limitations remain. Timings are development measurements, not an isolated repeated benchmark or realtime acceptance.',
 recipe:manifest.recipe,cases,diagnosis,diagnosticPrefix,paired_reference_wall_s:reference.stepping_wall_s,refined_reference_wall_s:refinedReference.stepping_wall_s,observed_paired_speedup:reference.stepping_wall_s/cases[0].wall_s,browser:browserResult,ui,artifacts},null,2));
console.log('Saved sample-reuse comparisons, failed strict refined gate, timings and browser evidence.');
