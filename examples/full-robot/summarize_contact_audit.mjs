import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const directory='runs/full-robot/learning/dynamics-breakdown',trial='runs/full-robot/learning/contact-pairs',browser='runs/interactive/dynamics-breakdown';
const artifacts={};
async function bytes(p){const b=await readFile(p);artifacts[p]=createHash('sha256').update(b).digest('hex');return b;}
async function read(p){return JSON.parse(await bytes(p));}
const sections=['frames','terminal_frame','hybrid_steps','hybrid_solves','contact_steps','contact_impulses'];
const basePath='runs/full-robot/learning/analytic-positions/base.execution.json';
const base=await read(basePath);assert(base.completed&&base.error===null);
const captures=[];
for(const name of ['profile','contact-profile','final']){
 const d=await read(`${directory}/${name}.execution.json`);assert(d.completed&&d.error===null);
 for(const k of sections)assert(k in d&&isDeepStrictEqual(d[k],base[k]),`${name}:${k}`);
 captures.push({name,wall_s:d.stepping_wall_s,exact:true,profile:d.solver_profile});
}
const exact=await read(trial+'/verification.json');assert(exact.passed);
for(const [p,h] of Object.entries(exact.inputs)){await bytes(p);assert.equal(artifacts[p],h);}
const benchmark=await read(trial+'/benchmark/manifest.json');assert.equal(benchmark.runs.length,4);
for(const [p,h] of Object.entries(benchmark.inputs)){await bytes(p);assert.equal(artifacts[p],h);}
for(const r of benchmark.runs){const d=await read(r.output);assert.equal(artifacts[r.output],r.sha256);for(const k of sections)assert(isDeepStrictEqual(d[k],base[k]));}
const normal=await read(trial+'/normal-only.execution.json');assert(normal.completed&&normal.error===null);for(const k of sections)assert(isDeepStrictEqual(normal[k],base[k]));
const screen=await read(trial+'/normal-only-screen.json'),normalResult=await read(trial+'/normal-only-result.json');assert.equal(normalResult.wall_s,normal.stepping_wall_s);assert(!normalResult.screen_passed&&normal.stepping_wall_s>screen.maximum_candidate_s);
const diagnosis=await read(directory+'/timestep-diagnosis.json');for(const [p,h] of Object.entries(diagnosis.inputs)){await bytes(p);assert.equal(artifacts[p],h);}
const portable=await read(browser+'/robot-browser.json'),ui=await read(browser+'/viewer-report.json');assert(portable.passed&&portable.compared_frames===281&&ui.passed&&ui.checks.length===22);
// Confirm the discarded distance-query implementation is absent from active source.
const sdf=await bytes('crates/sim-domain-robot/src/sdf.rs');assert(!sdf.toString().includes('sample_penetrating'));
for(const p of ['crates/sim-solve/src/profile.rs','crates/sim-domain-robot/src/articulated.rs','crates/sim-domain-robot/src/articulated/prepared.rs','crates/sim-domain-robot/src/articulated/embedding/dynamics.rs',
 'examples/interactive/prepare_exact_runtime.mjs','examples/interactive/verify_exact_runtime.mjs','examples/full-robot/diagnose_timestep_tracking.mjs','examples/full-robot/contact-query-validation.md',import.meta.filename,
 'runs/interactive/contact-pairs/normal-only-runner','runs/interactive/contact-pairs/pair-preparation.rs','runs/interactive/contact-pairs/normal-only-preparation.rs','runs/interactive/contact-pairs/normal-skipping.rs',
 browser+'/native-runner',browser+'/source-manifest.json',browser+'/viewer/build-manifest.json',browser+'/viewer/sim_web_bg.wasm'])await bytes(p);
await writeFile('examples/full-robot/contact-query-status.json',JSON.stringify({scope:'Retained instrumentation and comparison tools; both contact optimizations removed after low-yield timing results. No new physics/controller promotion.',
 contact_optimization_retained:false,training_model_accepted:false,realtime_accepted:false,captures,combined_trial:{verification:exact,benchmark},normal_trial:{screen,result:normalResult},timestep_diagnosis:diagnosis,browser:portable,viewer:ui,artifacts},null,2));
console.log(JSON.stringify({retained:'instrumentation and exact-comparison tools',combined_speedup:benchmark.result.speedup,normal_screen_passed:normalResult.screen_passed,browser:portable.passed,ui_checks:ui.checks.length,artifact_count:Object.keys(artifacts).length}));
