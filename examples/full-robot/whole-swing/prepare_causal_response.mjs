import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',output='runs/full-robot/learning/causal-response';
assert(!existsSync(output));mkdirSync(output,{recursive:true});
const read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const scenePath=`${root}/settled-integral-candidate.scene.json`,scene=read(scenePath);
const configPath=`${root}/direct-support-steering.config.json`,config=read(configPath);
const archivePath=`${root}/settled-integral-regression-recipes.json`,archive=read(archivePath);
for(const s of [archive.scene,archive.task])assert.equal(source(s.path).sha256,s.sha256);
const original=archive.cases.find(c=>c.name==='steering-1.25ms'),task=read(archive.task.path);
const priorPath=`${root}/direct-support-status.json`,prior=read(priorPath).cases.find(c=>c.name==='direct-steering');
assert(prior.completed&&prior.passed);const baseline=prior.sources.find(s=>s.path.endsWith('.native.json'));
assert.equal(source(baseline.path).sha256,baseline.sha256);
const stride=Math.round(task.period_s/config.step_s);let held=scene.controller.inputs.map(c=>c.initial),next=0;
const commanded=Array.from({length:config.steps/stride},(_,i)=>{
  if(original.input_events[next]?.at_step===i*stride)held=original.input_events[next++].values;
  return [...held];
});
assert.equal(next,original.input_events.length);
assert.equal(createHash('sha256').update(JSON.stringify(commanded)).digest('hex'),original.actions_json_sha256);
const probes=[{name:'hold-forward',stage:0,start_s:0,end_s:8.4,metric:'translation',direction:1},
  {name:'hold-turn',stage:1,start_s:8.4,end_s:16.8,metric:'yaw',direction:1},
  {name:'hold-reverse',stage:2,start_s:16.8,end_s:20,metric:'translation',direction:-1},
  {name:'hold-stop',stage:3,start_s:20,end_s:24,metric:'translation',direction:1}]
  .map(p=>({...p,body_link:config.policy.body_feedback.reference_link,threshold:p.metric==='yaw'?0.0005:0.0001,hold_s:0.1}));
const cases=[{name:'commanded'},...probes].map(def=>{
  const actions=structuredClone(commanded);
  if(def.name!=='commanded'){
    const begin=Math.round(def.start_s/task.period_s),end=Math.round(def.end_s/task.period_s);
    const previous=begin?commanded[begin-1]:scene.controller.inputs.map(c=>c.initial);
    assert.notDeepEqual(previous,commanded[begin]);
    for(let i=begin;i<end;i++)actions[i]=[...previous];
  }
  const actionPath=`${output}/${def.name}.actions.json`;writeFileSync(actionPath,JSON.stringify(actions)+'\n');
  return {...def,scene:scenePath,config:configPath,actions:actionPath,task:archive.task.path,
    duration_s:config.steps*config.step_s,step_s:config.step_s,seed:original.seed};
});
const plan={version:1,cases,baseline,probes,
  sources:[`${root}/CAUSAL-RESPONSE-PLAN.md`,scenePath,configPath,archivePath,priorPath,archive.task.path,
    ...cases.map(c=>c.actions),'examples/interactive/causal_response.mjs','examples/interactive/test_causal_response.mjs',
    `${root}/causal-response-browser-timeline-integrity.json`,`${root}/causal-response-browser-status.json`,
    'runs/causal-response-analyzer-tests.log','runs/load-damping-native-build.log',
    'target/release/examples/run_environment','target/release/examples/evaluate_lift',import.meta.filename].map(source),
  scope:'Five matched native steering runs: one exact repeat and four single-interval held-command counterfactuals. Sensitivity and dwell frozen before evaluation; no controller promotion, new accuracy or hardware claim.'};
for(const p of [`${output}/plan.json`,`${root}/causal-response-plan.json`])writeFileSync(p,JSON.stringify(plan,null,2)+'\n');
console.log(`Prepared ${cases.length} paired command-response runs.`);
