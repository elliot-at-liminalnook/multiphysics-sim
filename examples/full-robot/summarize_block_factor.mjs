import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const dir='runs/full-robot/learning/block-factor',browser='runs/interactive/block-factor',artifacts={};
async function bytes(p){const b=await readFile(p);artifacts[p]=createHash('sha256').update(b).digest('hex');return b;}
async function read(p){return JSON.parse(await bytes(p));}
const manifest=await read(dir+'/manifest.json');
const cases=[];
for(const prefix of ['','refined.']){
 const capture=await read(dir+'/'+prefix+'execution.json'),comparison=await read(dir+'/'+(prefix?'refined-comparison':'paired-comparison')+'.json'),lift=await read(dir+'/'+prefix+'lift.json');
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
 cases.push({strict_roundoff_gate_passed:strictRoundoffGate&&stateGate,step_s:capture.step_s,simulated_s:capture.simulated_s,wall_s:capture.stepping_wall_s,lift:lift.report,maximum_marker_difference_m:marker,maximum_contact_impulse_difference_ns:impulse,maximum_event_time_difference_s:event,maximum_sampled_differences:d,original_closure_maxima:closure,profile:capture.solver_profile});
}
const reference=await read(dir+'/reference.execution.json');
await read('runs/full-robot/learning/point-feedback/refined.execution.json');
const browserResult=await read(browser+'/robot-browser.json'),ui=await read(browser+'/viewer-report.json');assert(ui.passed&&browserResult.replay_exact&&browserResult.reset_exact);
for(const p of ['config.json','viewer.config.json','reference.config.json','refined.config.json','refinement.json'])await read(dir+'/'+p);
for(const p of ['examples/full-robot/block-factor-experiment.json','examples/full-robot/block-factor-validation.md','examples/full-robot/prepare_block_factor.mjs','examples/full-robot/summarize_block_factor.mjs','crates/sim-domain-robot/src/articulated/embedding.rs','crates/sim-domain-robot/src/articulated/embedding/factor_blocks.rs','crates/sim-domain-robot/tests/embedding.rs','crates/sim-runtime/examples/compare_embedding.rs',browser+'/native-runner',browser+'/source-manifest.json',browser+'/viewer/build-manifest.json',browser+'/viewer/sim_web_bg.wasm'])await bytes(p);
await writeFile('examples/full-robot/block-factor-status.json',JSON.stringify({milestone:'exact_independent_block_factorization',strict_roundoff_gate_passed:cases.every(c=>c.strict_roundoff_gate_passed),browser_parity_passed:browserResult.passed,promoted_default:false,task_accepted:false,
 scope:'Opt-in equivalent linear algebra. Base-step comparison meets strict roundoff gate; refined comparison exceeds it despite unchanged sampled lift. Existing controller/model limitations remain. Timings are development measurements, not an isolated repeated benchmark or realtime acceptance.',
 recipe:manifest.recipe,cases,paired_reference_wall_s:reference.stepping_wall_s,observed_paired_speedup:reference.stepping_wall_s/cases[0].wall_s,browser:browserResult,ui,artifacts},null,2));
console.log('Saved block-factor equivalence, timings and browser evidence.');
