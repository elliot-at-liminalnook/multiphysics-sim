import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/neural-teacher',browser='runs/interactive/neural-teacher';
const read=p=>JSON.parse(readFileSync(p));
const digest=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const evidence={};
const load=(key,path)=>{evidence[key]=digest(path);return read(path);};
const score=c=>c.transitions.reduce((s,t)=>s+t.reward,0);
const capture=name=>load(name,`${root}/${name}.native.json`);
const zero=capture('zero'),train=capture('train'),held=capture('heldout'),heldZero=capture('heldout-zero');
const fine=capture('heldout-refined'),fineZero=capture('heldout-zero-refined');
const sustained=capture('sustained-80');
const failures=['sustained','candidate-3-sustained','sustained-refined'].map(name=>{const r=capture(name);return {name,completed:r.completed,time_s:r.frames.at(-1).time_s,error:r.error};});
const search=load('search','examples/full-robot/neural-teacher/search.json');
const artifact=load('artifact',`${root}/artifact-report.json`);
const parity=load('parity',`${browser}/sustained-parity.json`);
const ui=load('ui',`${browser}/viewer-report.json`);
const timing=load('timing',`${browser}/live-performance.json`);
const bundle=load('bundle',`${browser}/viewer/build-manifest.json`);
const geometry={};
for(const name of ['train','heldout','heldout-refined','sustained-80','sustained-refined-80']){
 const r=load(`${name}_acceptance`,`${root}/${name}-acceptance/summary.json`);
 geometry[name]={passed:r.passed,swings:r.lifts.length,passed_swings:r.lifts.filter(l=>l.passed).length,
  final_body_error_m:r.final_body_error_m,maximum_body_tilt_rad:r.maximum_body_tilt_rad,final_yaw_error_rad:r.final_yaw_error_rad,
  inter_link_geometry_audit:r.inter_link_geometry_audit};
}
assert(artifact.passed&&parity.passed&&ui.passed&&timing.completed&&geometry['sustained-80'].passed);
assert.deepEqual(sustained.recording.config,read('examples/full-robot/neural-teacher/config.json'));
assert.equal(score(train),search.best_score);assert.equal(score(zero),search.initial_score);
assert.deepEqual(read('examples/full-robot/neural-teacher/policy.json'),search.policy);
for(const [path,sha] of Object.entries(bundle.inputs))assert.equal(digest(path).sha256,sha,`stale bundle ${path}`);
const rendered=load('keyboard_recording',`${browser}/live-performance.recording.json`);
assert.deepEqual(rendered.runtime.input_events,sustained.recording.input_events);
const refinement=load('sustained_refinement',`${root}/sustained-refinement.json`);
const p=search.policy;
const sourcePaths=['crates/sim-domain-control/src/neural.rs','crates/sim-domain-control/src/policy_search.rs','crates/sim-domain-control/tests/neural.rs',
 'crates/sim-runtime/src/embedded_policy.rs','crates/sim-runtime/src/embedded.rs','crates/sim-runtime/src/environment.rs',
 'crates/sim-runtime/examples/train_residual_policy.rs','crates/sim-runtime/tests/neural_policy.rs',
 'examples/full-robot/prepare_neural_teacher.mjs','examples/full-robot/materialize_neural_teacher.mjs','examples/full-robot/check_neural_teacher.mjs','examples/full-robot/summarize_neural_teacher.mjs',
 ...['scene','config','short.config','initial.config','task','experiment','policy','search','train.actions','heldout.actions','manifest'].map(n=>`examples/full-robot/neural-teacher/${n}.json`),
 ...Array.from({length:search.trials.length+1},(_,i)=>`examples/full-robot/neural-teacher/trials/evaluation-${String(i).padStart(4,'0')}.json`)];
const report={version:1,stage:'first experimental learned residual teacher; not a complete or deployable walking policy',sources:sourcePaths.map(digest),evidence,
 network:{features:p.features.length,hidden:p.layers[0].biases.length,outputs:p.outputs.length,parameters:p.layers.reduce((s,l)=>s+l.biases.length+l.weights.flat().length,0),maximum_checked_correction_rad:artifact.maximum_correction_rad},
 search:{seed:417,evaluations:search.trials.length+1,failed_candidates:search.trials.filter(t=>t.error).length,
  training_reward:{zero:score(zero),learned:score(train),difference:score(train)-score(zero)},
  heldout_reward:{zero:score(heldZero),learned:score(held),difference:score(held)-score(heldZero)},
  heldout_refined_reward:{zero:score(fineZero),learned:score(fine),difference:score(fine)-score(fineZero)},
  scope:'One seed, four paired random-search iterations, one 24 s training command sequence. Reverse-first commands excluded from training. Gains are tiny and do not establish a practically better or robust walking controller.'},
 geometry,refinement,failed_diagnostics:failures,native_wasm:parity,viewer:ui,
 rendered:{...timing,performance:{...timing.performance,breakdown:undefined}},
 runtime_tests:{library:2,runtime:9,result:'passed',logs:[digest(`${root}/library-tests.log`),digest(`${root}/runtime-tests.log`)]},
 remaining:['refined sustained final body error exceeds 1 mm','meaningful multi-scenario policy improvement','teacher-to-student distillation','bounded disturbance training','hardware calibration','command-to-visible-response latency',...(!timing.meets_transition_target?['20 ms rendered p95 transition target']:[])],
 scope:'Native/WASM execution, bounded learned action effects and nominal sampled stepping are demonstrated. Detailed physics, the baseline controller and failed comparisons remain available. No robustness, sim-to-real accuracy or full-goal completion claim.'};
writeFileSync('examples/full-robot/neural-teacher-status.json',JSON.stringify(report,null,2)+'\n');
console.log('examples/full-robot/neural-teacher-status.json');
