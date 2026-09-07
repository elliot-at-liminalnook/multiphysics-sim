import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/closure-preparation';
const browser='runs/interactive/closure-preparation';
const artifacts={};
async function bytes(p){const b=await readFile(p);artifacts[p]=createHash('sha256').update(b).digest('hex');return b;}
async function read(p){return JSON.parse(await bytes(p));}
const manifest=await read(root+'/manifest.json');
const sections=['frames','terminal_frame','hybrid_steps','hybrid_solves','contact_steps','contact_impulses'];
const cases=[];
for(const name of ['base','refined']){
 const d=await read(`${root}/${name}.execution.json`),p=await read(manifest.comparisons[name]);
 assert(d.completed&&d.error===null&&p.completed&&p.error===null);
 await bytes(`${root}/${name}.config.json`);
 const exact={};for(const k of sections){assert(k in d&&k in p);exact[k]=isDeepStrictEqual(d[k],p[k]);assert(exact[k],`${name}:${k}`);}
 cases.push({name,step_s:d.step_s,simulated_s:d.simulated_s,exact,wall_s:d.stepping_wall_s,
  wall_time_scope:'Development runs overlap other jobs; use the separate ABBA benchmark for performance.',
  profile:d.solver_profile,parent_profile:p.solver_profile});
}
const benchmark=await read(root+'/benchmark/manifest.json');assert.equal(benchmark.runs.length,4);
for(const run of benchmark.runs){
 const d=await read(run.output),p=await read(manifest.comparisons.base);
 assert(d.completed&&d.error===null);
 for(const k of sections)assert(isDeepStrictEqual(d[k],p[k]),`${run.mode}:${k}`);
 assert.equal(artifacts[run.output],run.sha256);
}
const portable=await read(browser+'/robot-browser.json'),ui=await read(browser+'/viewer-report.json');
assert(portable.passed&&portable.compared_frames===281&&ui.passed);
for(const p of [manifest.scene,manifest.previous_runner,browser+'/native-runner',browser+'/source-manifest.json',
 browser+'/viewer/build-manifest.json',browser+'/viewer/sim_web_bg.wasm',
 'crates/sim-domain-robot/src/articulated/constraints.rs','crates/sim-domain-robot/src/articulated/embedding.rs',
 'crates/sim-domain-robot/tests/constraint_audit.rs','crates/sim-domain-robot/tests/embedding.rs','examples/full-robot/prepare_closure_preparation.mjs',
 'examples/interactive/benchmark_exact_runtime.mjs',import.meta.filename,
 'examples/full-robot/closure-preparation-validation.md',
 'runs/interactive/robot-lab-closure-preparation-2026-09-07.zip'])await bytes(p);
const status={scope:'Closure preparation optimization in the shared library: omit numeric-row labels, cache fixed units and omit unused SVD vectors for QR. All original equations and rank checks remain. Parent model discrepancies and walking/learning gates remain open.',
  native_equivalence_passed:true,training_model_accepted:false,realtime_accepted:false,
  cases,benchmark,browser:portable,viewer:ui,artifacts};
await writeFile('examples/full-robot/closure-preparation-status.json',JSON.stringify(status,null,2));
console.log(JSON.stringify({exact_cases:cases.map(c=>c.name),benchmark:benchmark.result,browser:portable.passed,viewer_checks:ui.checks.length,artifact_count:Object.keys(artifacts).length}));
