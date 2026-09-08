import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',definition=`${root}/fast-distillation`,run='runs/full-robot/learning/fast-distillation-evaluation';
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const statusPath=`${root}/fast-distillation-evaluation-status.json`,status=read(statusPath);assert(status.complete);
const integrityPath=`${root}/fast-distillation-evaluation-integrity.json`;assert(read(integrityPath).passed);
const manifest=read(`${definition}/manifest.json`);for(const s of manifest.inputs)assert.equal(source(s.path).sha256,s.sha256);
for(const s of read(`${root}/fast-distillation-fit-inputs.json`).sources)assert.equal(source(s.path).sha256,s.sha256);
const fit=read(`${definition}/fit.json`),validation=read(`${definition}/validation.json`);
assert.equal(fit.best_training_loss,Math.min(fit.initial_loss,...fit.training_loss));assert.deepEqual(fit.policy,read(`${definition}/policy.json`));
const sources=[statusPath,integrityPath,`${root}/fast-distillation-fit-inputs.json`,`${definition}/manifest.json`,`${definition}/fit.json`,`${definition}/policy.json`,`${definition}/validation.json`,`${definition}/observation-boundary.json`,`${root}/fast-distillation-holdout-status.json`,`${root}/fast-distillation-holdout-integrity.json`,'runs/fast-distillation-dataset-tests.log','runs/fast-distillation-neural-tests.log',import.meta.filename];
const cases=[];
for(const c of status.cases){
  const capture=c.sources.find(s=>s.path.endsWith('.native.json')),path=`${run}/${c.name}.metrics.json`;
  execFileSync(process.execPath,['examples/interactive/analyze_walking_capture.mjs',capture.path,path,...(!c.completed?['--accepted-prefix']:[])],{stdio:'ignore'});
  const metrics=read(path);assert.equal(metrics.capture.sha256,capture.sha256);sources.push(path);
  cases.push({...c,metrics});
}
const result={version:1,fit:{training_samples:manifest.training_samples,validation_samples:manifest.validation_samples,selected_by:'training loss only',selected_epoch:fit.best_epoch,
  initial_training_loss:fit.initial_loss,final_training_loss:fit.best_training_loss,validation},corpus:manifest.inventory,cases,observation_boundary:read(`${definition}/observation-boundary.json`),
  tests:{typed_dataset:3,shared_neural:4},sources:sources.map(source),
  scope:'Faster fine-step teacher-to-student motor residual learning. Development and predeclared held-out closed-loop outcomes are retained independently of imitation error. Validation uses only the accepted prefix of the failed teacher episode; that prefix cannot qualify full held-out walking. Detailed accuracy refinement, browser realtime, harder terrain, robustness training and hardware calibration remain separate.'};
writeFileSync(`${root}/fast-distillation-summary.json`,JSON.stringify(result,null,2)+'\n');
console.log(cases.map(c=>({name:c.name,passed:c.passed,time:c.metrics.simulated_s,speed_mm_s:c.metrics.sustained_windows[0]?.measured_sustained_speed_m_s*1000,work_j:c.metrics.positive_mechanical_work_j,error:c.error})));
