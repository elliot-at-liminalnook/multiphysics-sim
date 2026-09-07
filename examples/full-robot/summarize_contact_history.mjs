import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/contact-history';
const browser='runs/interactive/contact-history';
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
 'crates/sim-domain-robot/src/articulated.rs','crates/sim-domain-robot/src/articulated/prepared.rs',
 'crates/sim-domain-robot/src/articulated/friction.rs','crates/sim-domain-robot/src/articulated/embedding/implicit.rs',
 'crates/sim-domain-robot/tests/jacobian.rs','examples/full-robot/prepare_contact_history.mjs',
 'examples/full-robot/benchmark_contact_history.mjs',import.meta.filename,
 'examples/full-robot/contact-history-validation.md',
 'runs/interactive/robot-lab-contact-history-2026-09-07.zip'])await bytes(p);
const status={scope:'Dependency-exact contact-history query optimization in the shared library. Detailed physical laws and all numerical acceptance rules are unchanged. Parent model discrepancies and walking/learning gates remain open.',
  native_equivalence_passed:true,training_model_accepted:false,realtime_accepted:false,
  cases,benchmark,browser:portable,viewer:ui,artifacts};
await writeFile('examples/full-robot/contact-history-status.json',JSON.stringify(status,null,2));
console.log(JSON.stringify({exact_cases:cases.map(c=>c.name),benchmark:benchmark.result,browser:portable.passed,viewer_checks:ui.checks.length,artifact_count:Object.keys(artifacts).length}));
