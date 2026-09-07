import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/student-distillation',browser='runs/interactive/student-distillation',definition='examples/full-robot/student-distillation';
const read=p=>JSON.parse(readFileSync(p));
const hash=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const evidence={};const load=(key,path)=>{evidence[key]=hash(path);return read(path);};
const validation=load('fit_validation',`${definition}/validation.json`);
const fit=load('fit',`${definition}/fit.json`),experiment=load('experiment',`${definition}/experiment.json`);
assert.deepEqual(fit.policy,read(`${definition}/policy.json`));
assert.equal(fit.best_training_loss,validation.final_training_loss);
assert.equal(fit.best_training_loss,Math.min(fit.initial_loss,...fit.training_loss));
assert(validation.final_validation_loss<validation.initial_validation_loss);
const cases={};
for(const name of ['first','train','heldout','sustained','multi-train','multi-heldout','multi-sustained']){
 const r=load(name,`${root}/${name}-acceptance/summary.json`);
 cases[name]={passed:r.passed,swings:r.lifts.length,passed_swings:r.lifts.filter(l=>l.passed).length,final_body_error_m:r.final_body_error_m,maximum_body_tilt_rad:r.maximum_body_tilt_rad,final_yaw_error_rad:r.final_yaw_error_rad,inter_link_geometry_audit:r.inter_link_geometry_audit};
}
const unused=load('unused_work',`${root}/multi-unused-feedback-report.json`);
const parity=load('parity',`${browser}/sustained-parity.json`),ui=load('viewer',`${browser}/viewer-report.json`),timing=load('timing',`${browser}/live-performance.json`);
const bundle=load('bundle',`${browser}/viewer/build-manifest.json`);
for(const [path,sha] of Object.entries(bundle.inputs))assert.equal(hash(path).sha256,sha,`stale bundle ${path}`);
const capture=load('sustained_capture',`${root}/multi-sustained.native.json`);
assert.deepEqual(capture.recording.config,read(`${definition}/config.json`));
const recording=load('keyboard_recording',`${browser}/live-performance.recording.json`);
assert.deepEqual(recording.runtime.input_events,capture.recording.input_events);
assert(unused.passed&&parity.passed&&ui.passed&&timing.completed&&cases['multi-train'].passed&&cases['multi-heldout'].passed);
const sourcePaths=['crates/sim-core/src/couple.rs','crates/sim-domain-control/src/neural.rs','crates/sim-domain-control/src/distillation.rs','crates/sim-domain-control/tests/neural.rs','crates/sim-runtime/examples/distill_policy.rs','crates/sim-runtime/src/embedded_policy.rs',
 'examples/full-robot/prepare_student_distillation.mjs','examples/full-robot/materialize_student.mjs','examples/full-robot/check_unused_feedback.mjs','examples/full-robot/summarize_student_distillation.mjs',
 ...['scene','task','config','short.config','policy','manifest','observation-boundary'].map(n=>`${definition}/${n}.json`)];
const report={version:1,stage:'experimental distilled motor-feedback student',sources:sourcePaths.map(hash),evidence,
 network:{features:fit.policy.features.length,hidden:fit.policy.layers[0].biases.length,outputs:fit.policy.outputs.length,parameters:fit.policy.layers.reduce((n,l)=>n+l.weights.flat().length+l.biases.length,0)},
 optimizer:experiment.optimizer,validation,cases,unused_work:unused,native_wasm:parity,viewer:ui,rendered:{...timing,performance:{...timing.performance,breakdown:undefined}},
 observation_boundary:read(`${definition}/observation-boundary.json`),
 tests:{neural:4,runtime:9,logs:[hash(`${root}/neural-tests.log`),hash(`${root}/runtime-tests.log`)]},
 sustained_task_accepted:cases['multi-sustained'].passed,
 remaining:['sustained stopping error exceeds 1 mm','bounded perturbation training and held-out robustness','actual sensor definitions and causal planner state estimation','timestep sensitivity for student controller','command-to-visible-response latency',...(!timing.meets_transition_target?['20 ms rendered p95 transition target']:[])],
 scope:'Teacher-to-student initialization and live native/WASM execution are implemented. Short task acceptance and exact omission of unused calculations pass; full-minute stopping, robustness and hardware deployment remain unproven or failed. The original goal remains active.'};
writeFileSync('examples/full-robot/student-distillation-status.json',JSON.stringify(report,null,2)+'\n');console.log('examples/full-robot/student-distillation-status.json');
