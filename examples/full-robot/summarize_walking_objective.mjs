import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/walking-objective',prior='runs/full-robot/learning/student-disturbances',browser='runs/interactive/walking-objective',definition='examples/full-robot/walking-objective';
const read=p=>JSON.parse(readFileSync(p));
const hash=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const walking=read(`${definition}/walking.json`),cases={};
for(const name of ['lateral','stronger-lateral','stress-5','stress-15','refined','unforced-refined','refined-5ms']){
 const p=`${root}/${name}.rescore.json`,r=read(p),offline=read(`${prior}/${name}-acceptance/summary.json`);
 assert.deepEqual(r.config,walking);assert.equal(r.outcomes.length,offline.lifts.length);
 for(const o of r.outcomes){const expected=offline.lifts.find(l=>l.step===o.step);assert(expected);const {step,foot,...lift}=expected;assert.equal(o.foot,foot);assert.deepEqual(o.lift,lift);assert.equal(o.passed,lift.passed);}
 cases[name]={...r,source:hash(p),original_capture:hash(`${prior}/${name}.native.json`),offline:hash(`${prior}/${name}-acceptance/summary.json`),physical_task_accepted:offline.passed};
}
const accepted=Object.values(cases).filter(c=>c.physical_task_accepted),failed=Object.values(cases).filter(c=>!c.physical_task_accepted);
assert(Math.min(...accepted.map(c=>c.total_reward))>Math.max(...failed.map(c=>c.total_reward)));
const exact=['lateral','stress-15'].map(n=>read(`${root}/${n}.check.json`));assert(exact.every(r=>r.passed));
const candidates={};
for(const name of ['lift-6mm','lift-6mm-stronger','lift-6mm-refined','lift-6mm-heldout','lift-6mm-sustained','lift-6mm-5ms']){
 const p=`${root}/${name}.native.json`,r=read(p),last=r.transitions.at(-1),configName=name==='lift-6mm-heldout'?'lift-6mm-refined':name;
 assert.deepEqual(r.recording.config,read(`${definition}/${configName}.config.json`));
 const a=r.completed?read(`${root}/${name}-acceptance/summary.json`):null;
 candidates[name]={capture:hash(p),config:hash(`${definition}/${configName}.config.json`),completed:r.completed,error:r.error,
  committed_time_s:r.frames.at(-1).time_s,eligible_completed_episode:r.completed&&r.error===null,
  qualified:last.walking.qualified_steps,failed_swings:last.walking.failed_steps,final_body_error_m:Math.hypot(...last.walking.body_error_world_m),
  independent_task_accepted:a?.passed??false,independent_acceptance:a?hash(`${root}/${name}-acceptance/summary.json`):null,
  shortest_qualified_span_s:a?Math.min(...a.lifts.map(l=>l.longest_qualifying_span_s)):null,
  total_reward:r.completed?r.transitions.reduce((n,t)=>n+t.reward,0):null};
}
const parity=read(`${browser}/parity.json`),ui=read(`${browser}/viewer-report.json`),timing=read(`${browser}/live-performance.json`);
assert(parity.passed&&ui.passed&&timing.completed);
assert.deepEqual(read(`${browser}/live-performance.recording.json`).runtime.input_events,read(`${root}/lateral.native.json`).recording.input_events);
const bundle=read(`${browser}/viewer/build-manifest.json`);for(const [p,sha] of Object.entries(bundle.inputs))assert.equal(hash(p).sha256,sha,`stale bundle: ${p}`);
const sources=['crates/sim-runtime/src/walking_task.rs','crates/sim-runtime/src/environment.rs','crates/sim-runtime/src/embedded.rs','crates/sim-runtime/src/lift.rs','crates/sim-runtime/examples/rescore_walking.rs','crates/sim-runtime/tests/walking_task.rs','examples/full-robot/check_walking_objective.mjs','examples/full-robot/summarize_walking_objective.mjs',`${definition}/walking.json`,`${definition}/task.json`,'web/viewer/viewer.js','web/viewer/index.html','web/viewer/viewer.css','web/tests/viewer.mjs'];
const report={version:1,stage:'executed walking objective and reference-margin investigation',sources:sources.map(hash),walking,
 neural_training_performed:false,controller_promoted:false,cases,known_failure_ranking_correct:true,exact_physics_checks:exact,reference_candidates:candidates,
 native_wasm:parity,viewer:ui,rendered:{...timing,performance:{...timing.performance,breakdown:undefined}},bundle:hash(`${browser}/viewer/build-manifest.json`),
 tests:{focused_and_existing:23,rescore_cli:read(`${root}/rescore-cli-check.json`),logs:['tests','workspace-check','wasm-build','rescore-build'].map(n=>hash(`${root}/${n}.log`))},
 remaining:['closed-loop learning with the corrected task','reference candidate sustained stopping exceeds 1 mm','reference candidate 5 ms solve fails near 17.005 seconds','bounded disturbance training and independent robustness evaluation','causal sensor/estimator observations and physical calibration','20 ms p95 transition and command-to-visible-response acceptance'],
 scope:'New task code leaves recorded physics and actor observations unchanged, distinguishes the observed failures, and runs in WASM with visible outcome diagnostics. It is not a trained controller improvement, proof against reward exploitation, or a completion of the full active goal.'};
writeFileSync('examples/full-robot/walking-objective-status.json',JSON.stringify(report,null,2)+'\n');console.log({ranking:report.known_failure_ranking_correct,cases:Object.keys(cases).length,candidates:Object.keys(candidates).length,rendered:timing.performance.active_motion});
