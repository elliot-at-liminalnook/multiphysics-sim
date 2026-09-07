import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const directory='runs/full-robot/learning/analytic-positions';
const browser='runs/interactive/analytic-positions';
const artifacts={};
async function bytes(p){const b=await readFile(p);artifacts[p]=createHash('sha256').update(b).digest('hex');return b;}
async function read(p){return JSON.parse(await bytes(p));}
const manifest=await read(directory+'/manifest.json');
for(const [p,h] of Object.entries({...manifest.inputs,...manifest.outputs})) {await bytes(p);assert.equal(artifacts[p],h);}
const geometry=await read(directory+'/geometry-audit.json');
assert(geometry.comparison.passed&&geometry.issues.length===0&&geometry.dependent_count===geometry.covered_count);
const cases=[];
for(const name of ['base','refined']){
 const d=await read(`${directory}/${name}.execution.json`),c=await read(`${directory}/${name}.comparison.json`);
 const parent=await read(manifest.comparisons[name]);
 assert(d.completed&&d.error===null&&d.embedding.analytic_mechanism_positions);
 const marker=Math.max(...c.markers.map(x=>x.maximum_error_m));
 const impulse=Math.max(...c.contact_impulse_comparison.total.map(x=>x.difference_norm_ns));
 const event=Math.max(...c.events.map(x=>x.maximum_ordered_time_difference_s));
 const state=c.maximum_sampled_differences;
 const strict=marker<1e-9&&impulse<1e-7&&event<1e-8&&state.joint_positions<1e-8&&state.joint_velocities<1e-6&&state.current_a<1e-7&&state.motor_voltage_v<1e-6&&state.shaft_torque_nm<1e-7&&c.events.every(x=>x.candidate_count===x.reference_count)&&c.contact_pair_mismatch_samples===0;
 assert(strict,`${name} failed existing numerical comparison gates`);
 const closure={};
 for(const f of d.frames)for(const row of f.original_rows){
  const scale=row.unit==='m'?d.embedding.length_scale_m:row.unit==='rad'?d.embedding.angle_scale_rad:1;
  const maximum=closure[row.unit]??={position:0,velocity:0,acceleration:0};
  for(const key of Object.keys(maximum)){assert(Number.isFinite(row[key]));maximum[key]=Math.max(maximum[key],Math.abs(row[key]));assert(Math.abs(row[key])/scale<=d.embedding.scaled_closure_tolerance);}
 }
 const lift=(await read(`${directory}/${name}.lift.json`)).report;assert(lift.passed);
 cases.push({name,strict_numerical_comparison_passed:strict,step_s:d.step_s,simulated_s:d.simulated_s,wall_s:d.stepping_wall_s,
  timing_scope:'Overlapping development jobs; use isolated ABBA for speedup.',maximum_marker_difference_m:marker,maximum_contact_impulse_difference_ns:impulse,maximum_event_time_difference_s:event,
  maximum_sampled_differences:state,original_closure_maxima:closure,lift,
  rejected_trials:d.hybrid_steps.reduce((n,s)=>n+s.rejected_trials,0),parent_rejected_trials:parent.hybrid_steps.reduce((n,s)=>n+s.rejected_trials,0),profile:d.solver_profile,parent_profile:parent.solver_profile});
}
const baselineBrowser=await read(browser+'/robot-browser.json');
const candidateBrowser=await read(browser+'/analytic-browser.json');
const ui=await read(browser+'/candidate-viewer-report.json');
assert(baselineBrowser.passed&&candidateBrowser.passed&&ui.passed);
assert.equal(candidateBrowser.compared_frames,281);
assert(ui.checks.some(x=>x.startsWith('robot-point-analytic-positions ')));
const benchmark=await read(directory+'/benchmark/manifest.json');assert.equal(benchmark.runs.length,4);
for(const run of benchmark.runs){
 const d=await read(run.output),expected=await read(run.reference);
 assert.equal(artifacts[run.output],run.sha256);
 for(const key of ['frames','terminal_frame','hybrid_steps','hybrid_solves','contact_steps','contact_impulses'])assert(key in d&&key in expected&&isDeepStrictEqual(d[key],expected[key]));
}
for(const p of [manifest.scene,browser+'/native-runner',browser+'/source-manifest.json',browser+'/candidate-viewer/build-manifest.json',browser+'/candidate-viewer/sim_web_bg.wasm',
 'crates/sim-domain-robot/src/articulated/embedding.rs','crates/sim-domain-robot/src/articulated/embedding/slider_crank.rs','crates/sim-domain-robot/tests/embedding.rs',
 'crates/sim-runtime/examples/audit_analytic_mechanisms.rs','crates/sim-runtime/examples/compare_embedding.rs','examples/interactive/benchmark_exact_runtime.mjs',
 'web/viewer/presets.json','web/tests/viewer.mjs','examples/full-robot/analytic-positions-validation.md',import.meta.filename])await bytes(p);
const status={scope:'Opt-in direct mechanism positions; numeric tangent/curvature/rank and all original closure checks retained. Existing task/model/calibration gaps remain.',
 promoted_default:false,training_model_accepted:false,realtime_accepted:false,geometry:{...geometry,comparison:{...geometry.comparison,samples:undefined}},cases,
 baseline_browser:baselineBrowser,candidate_browser:candidateBrowser,viewer:ui,benchmark,artifacts};
await writeFile('examples/full-robot/analytic-positions-status.json',JSON.stringify(status,null,2));
console.log(JSON.stringify({native_cases:cases.map(x=>({name:x.name,passed:x.strict_numerical_comparison_passed})),browser:candidateBrowser.passed,ui_checks:ui.checks.length,benchmark:benchmark.result,artifacts:Object.keys(artifacts).length}));
