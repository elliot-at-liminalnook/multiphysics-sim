import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/endpoint-correction';
const browser='runs/interactive/endpoint-correction';
const artifacts={};
async function bytes(p){const b=await readFile(p);artifacts[p]=createHash('sha256').update(b).digest('hex');return b;}
async function read(p){return JSON.parse(await bytes(p));}
function comparison(c){return {
  maximum_sampled_differences:c.maximum_sampled_differences,
  maximum_marker_difference_m:Math.max(...c.markers.map(m=>m.maximum_error_m)),
  maximum_contact_impulse_difference_ns:Math.max(...c.contact_impulse_comparison.total.map(m=>m.difference_norm_ns)),
  maximum_ordered_event_difference_s:Math.max(0,...c.events.map(e=>e.maximum_ordered_time_difference_s??0)),
  event_counts_match:c.events.every(e=>e.candidate_count===e.reference_count),
  sampled_contact_pair_mismatches:c.contact_pair_mismatch_samples,
};}
const manifest=await read(directory+'/manifest.json');
const cases=[];
for(const name of ['base','refined']){
  const d=await read(`${directory}/${name}.execution.json`);
  assert(d.completed&&d.error===null&&d.implicit.auxiliary_endpoint_correction_scale);
  const parent=await read(manifest.comparisons[name]);
  const closure={};
  for(const f of d.frames)for(const row of f.original_rows){
    const scale=row.unit==='m'?d.embedding.length_scale_m:row.unit==='rad'?d.embedding.angle_scale_rad:1;
    const maxima=closure[row.unit]??={position:0,velocity:0,acceleration:0};
    for(const k of Object.keys(maxima)){
      assert(Number.isFinite(row[k]));maxima[k]=Math.max(maxima[k],Math.abs(row[k]));
      assert(Math.abs(row[k])/scale<=d.embedding.scaled_closure_tolerance);
    }
  }
  const work={};for(const s of d.hybrid_solves)for(const [k,v] of Object.entries(s))if(k.startsWith('successful_'))work[k]=(work[k]??0)+v;
  const maximumAuxiliaryResidual=Math.max(...d.hybrid_solves.map(s=>s.maximum_auxiliary_residual));
  assert(maximumAuxiliaryResidual<=d.implicit.newton.absolute_tolerance);
  await bytes(`${directory}/${name}.config.json`);
  cases.push({name,step_s:d.step_s,simulated_s:d.simulated_s,wall_s:d.stepping_wall_s,
    wall_time_scope:'Development run overlapped other assistant jobs; not an isolated speedup measurement.',
    original_closure_maxima:closure,maximum_auxiliary_residual:maximumAuxiliaryResidual,successful_trial_work:work,
    solver_profile:d.solver_profile,
    parent_rejected_trials:parent.hybrid_steps.reduce((n,s)=>n+s.rejected_trials,0),
    rejected_trials:d.hybrid_steps.reduce((n,s)=>n+s.rejected_trials,0),
    rejection_details:d.hybrid_steps.flatMap(s=>s.rejection_details),
    versus_parent:comparison(await read(`${directory}/${name}.comparison.json`)),
    versus_simultaneous_reference:comparison(await read(`${directory}/${name}.reference-comparison.json`)),
    lift:(await read(`${directory}/${name}.lift.json`)).report,
  });
}
const prefixes=[];
for(const mode of ['ordinary','endpoint']){
  const d=await read(`${directory}/prefix.${mode}.execution.json`);
  assert.equal(d.completed_steps,240);assert.equal(d.requested_steps,240);
  assert.equal(d.completed,false);assert(d.error.startsWith('simulation horizon ended before motion completed:'));
  await bytes(`${directory}/prefix.${mode}.config.json`);
  prefixes.push({mode,rejections:d.hybrid_steps.flatMap(s=>s.rejection_details)});
}
const portable=await read(browser+'/pendulum-browser.json');
const ui=await read(browser+'/viewer-report.json');assert(portable.passed&&ui.passed);
for(const p of [manifest.scene,browser+'/native-runner',browser+'/source-manifest.json',browser+'/pendulum.execution.json',browser+'/viewer/build-manifest.json',browser+'/viewer/sim_web_bg.wasm',
 'crates/sim-solve/src/lib.rs','crates/sim-solve/src/coloring.rs','crates/sim-solve/tests/coloring.rs',
 'crates/sim-domain-robot/src/articulated/embedding/implicit.rs','crates/sim-domain-robot/tests/embedded_motor.rs',
 'crates/sim-runtime/tests/embedded_session.rs','examples/interactive/pendulum.condensed.json',
 'web/tests/viewer.mjs','web/viewer/presets.json',import.meta.filename,'examples/full-robot/prepare_endpoint_correction.mjs',
 'examples/full-robot/endpoint-correction-validation.md',
 'runs/interactive/robot-lab-endpoint-correction-2026-09-07.zip'])await bytes(p);
const status={scope:'Endpoint-coordinate correction experiment. All original residual/closure checks retained; correction acceptance changes explicitly. Full training-model and controller task acceptance remain open.',
 promoted_default:false,training_model_accepted:false,cases,prefixes,
 prefix_divergence:await read(directory+'/prefix.divergence.json'),browser_fixture:portable,viewer:ui,artifacts};
await writeFile('examples/full-robot/endpoint-correction-status.json',JSON.stringify(status,null,2));
console.log(JSON.stringify(cases.map(c=>({name:c.name,wall_s:c.wall_s,rejected_trials:c.rejected_trials,parent_rejected_trials:c.parent_rejected_trials,parent:c.versus_parent,reference:c.versus_simultaneous_reference,lift:c.lift})),null,2));
