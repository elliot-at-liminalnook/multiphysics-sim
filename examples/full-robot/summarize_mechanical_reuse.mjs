import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const root='runs/full-robot/learning/mechanical-reuse',source='examples/full-robot/mechanical-reuse';
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const cases=[];
const definitions=[['short','neural-teacher/train.actions.json'],['sustained','browser-residual-policy/sustained.actions.json'],['refined','neural-teacher/train.actions.json'],['reverse','neural-teacher/heldout.actions.json'],['minute-push','browser-residual-policy/sustained.actions.json']];
for(const [name,actions] of [...definitions,['refined-minute','browser-residual-policy/sustained.actions.json'],...definitions.map(([n,a])=>['restart-'+n,a])]) {
  const configPath=`${source}/${name}.config.json`,acceptancePath=`${root}/${name}-acceptance/summary.json`;
  const config=read(configPath),acceptance=read(acceptancePath);
  assert(config.implicit.reuse_step_jacobian&&config.implicit.reuse_controller_sample_jacobian);
  assert.deepEqual(config.policy.neural_residual,read('examples/full-robot/student-robustness/policy.json'));
  const profilePath=`${root}/${name}.profile.json`;
  let work=null;
  if(fs.existsSync(profilePath)){
    const p=read(profilePath);assert(p.completed);
    work={profile_sha256:hash(profilePath),accepted_segments:p.accepted_implicit_steps.length,
      accepted_endpoint_evaluations:p.accepted_implicit_steps.reduce((s,d)=>s+d.endpoint_evaluations,0),
      started_with_reused_jacobian:p.accepted_implicit_steps.filter(d=>d.started_with_reused_jacobian).length,
      fresh_restarts:p.accepted_implicit_steps.filter(d=>d.fresh_restart_reason).length,
      fresh_restart_reasons:p.accepted_implicit_steps.filter(d=>d.fresh_restart_reason).map(d=>d.fresh_restart_reason),
      jacobian_builds:p.buckets.find(b=>b.name==='jacobian assembly').calls,
      rejected_trials:p.accepted_intervals.reduce((s,d)=>s+d.rejected_trials,0),
      minimum_accepted_step_s:Math.min(...p.accepted_intervals.map(d=>d.minimum_accepted_step_s)),
      recovered_intervals:p.accepted_intervals.filter(d=>d.rejected_trials>0)};
    assert(work.started_with_reused_jacobian>0);
  }
  cases.push({name,config:configPath,config_sha256:hash(configPath),actions:`examples/full-robot/${actions}`,
    actions_sha256:hash(`examples/full-robot/${actions}`),passed:acceptance.passed,acceptance,work});
}
const comparisons=['sustained','baseline-reference','reuse-reference','restart-sustained'].map(n=>read(`${source}/${n}-comparison.json`));
const report={version:1,baseline_commit:'35b751911b226e186cd7ceefa2f0ef90dc720ca2',
  scope:'Opt-in cross-step derivative reuse for the paced effective-servo profile, retaining both the initial trial and bounded fresh-restart revision. Same robot, force laws, policy weights and nominal tolerance. Additional subdivisions change finite-step trajectories in the initial trial; fresh restart removes them in the 20 ms unforced minute. Prior development cases plus an unforced 5 ms reference are retained; neither pushed-minute nor refined-minute heading passes. Concurrent profiled timings are not speed acceptance.',
  cases,comparisons,
  sources:['crates/sim-domain-robot/src/articulated/embedding/mechanical_advance.rs','crates/sim-domain-robot/src/articulated/embedding/implicit.rs','crates/sim-runtime/src/embedded.rs','examples/full-robot/student-distillation/scene.json','examples/full-robot/walking-objective/task.json'].map(path=>({path,sha256:hash(path)}))};
fs.writeFileSync(`${source}/study-status.json`,JSON.stringify(report,null,2)+'\n');
console.log(cases.map(c=>({name:c.name,passed:c.passed,jacobian_builds:c.work?.jacobian_builds})));
