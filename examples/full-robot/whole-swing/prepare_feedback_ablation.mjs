import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',output='runs/full-robot/learning/feedback-ablation';
assert(!existsSync(output));mkdirSync(output,{recursive:true});
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const priorPlanPath=`${root}/loaded-foot-damping-plan.json`,priorPlan=read(priorPlanPath);
const priorStatusPath=`${root}/loaded-foot-damping-status.json`,priorStatus=read(priorStatusPath);
const original=priorPlan.cases.find(c=>c.name==='position-only'),baseline=priorStatus.cases.find(c=>c.name===original.name);
for(const s of baseline.sources)assert.equal(source(s.path).sha256,s.sha256);
for(const path of ['target/release/examples/run_environment','target/release/examples/evaluate_lift']){
  assert.equal(source(path).sha256,priorPlan.sources.find(s=>s.path===path).sha256,'retain baseline executable');
}
const scene=read(original.scene),actions=read(original.actions);
const motion=scene.controller.parameters.motion_command_channels.map(name=>scene.controller.inputs.findIndex(i=>i.name===name));
assert(motion.every(i=>i>=0));
const cases=[['no-body',['command.body_gain']],['no-point',['command.point_gain']],['angles-only',['command.body_gain','command.point_gain']]].map(([name,disabled])=>{
  const indices=disabled.map(name=>scene.controller.inputs.findIndex(i=>i.name===name));assert(indices.every(i=>i>=0));
  const changed=actions.map(a=>{const copy=[...a];if(motion.some(i=>a[i]!==0))for(const i of indices)copy[i]=0;return copy;});
  const actionPath=`${output}/${name}.actions.json`;writeFileSync(actionPath,JSON.stringify(changed)+'\n');
  return {name,disabled_moving_gain_channels:disabled,scene:original.scene,config:original.config,task:original.task,
    actions:actionPath,duration_s:original.duration_s,step_s:original.step_s,seed:original.seed};
});
const plan={version:1,cases,baseline,baseline_case:original,maximum_contact_motion_to_body_advance_ratio:0.05,
  sources:[`${root}/FEEDBACK-ABLATION-PLAN.md`,`${root}/moving-feedback-baseline.json`,priorPlanPath,priorStatusPath,
    original.scene,original.config,original.task,original.actions,...cases.map(c=>c.actions),
    'examples/interactive/analyze_feedback_contributions.mjs','examples/interactive/feedback_contributions.mjs',
    'examples/interactive/test_feedback_contributions.mjs','runs/feedback-contributions-tests.log',
    'target/release/examples/run_environment','target/release/examples/evaluate_lift',import.meta.filename].map(source),
  scope:'Three frozen moving-feedback ablations against a pinned existing baseline. Only selected gain input channels change during nonzero motion requests; original stopping feedback, robot, physics and all other inputs are retained.'};
for(const path of [`${output}/plan.json`,`${root}/feedback-ablation-plan.json`])writeFileSync(path,JSON.stringify(plan,null,2)+'\n');
console.log(`Prepared ${cases.length} moving-feedback ablations.`);
