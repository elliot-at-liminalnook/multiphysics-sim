// Compare completed native trials; keep failed/pending cases explicit.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/measured-actuator-integration/gait-generation';
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
function load(name) {
  const dir=`${root}/${name==='baseline'?'full-authority':`robustness/${name}`}/pilot-journal`;
  const file=fs.readdirSync(dir).filter(f=>/^state-\d{20}\.json$/.test(f)).sort().at(-1);
  const path=`${dir}/${file}`,state=JSON.parse(fs.readFileSync(path));
  return {name,path,sha256:sha(path),experiment:state.journal.experiment,trial:state.journal.trials[0]};
}
const names=['baseline','delay-10ms','backlash-05deg','motor-spread','battery-chain'];
const base=load('baseline'), baseline=base.experiment.spec;
const physical=robot=>{const c=structuredClone(robot);delete c.actuator_profiles;delete c.source;return c;};
function config(value) {
  const c=structuredClone(value);
  const visit=x=>{if(x&&typeof x==='object')for(const k of Object.keys(x)){
    if(k==='expected_cad_sha256')delete x[k];else visit(x[k]);
  }};
  visit(c);delete c.motors.power;return c;
}
const cases=names.map(name=>{
  const run=load(name),s=run.experiment.spec;
  for(const k of ['task','source_actions','baseline','seed','parameterization','objective'])assert.deepEqual(s[k],baseline[k],k);
  assert.deepEqual(config(s.config),config(baseline.config));
  assert.deepEqual(physical(s.scene.robot),physical(baseline.scene.robot));
  const sceneWithoutRobot=scene=>{const c=structuredClone(scene);delete c.robot;return c;};
  assert.deepEqual(sceneWithoutRobot(s.scene),sceneWithoutRobot(baseline.scene));
  assert.deepEqual(run.experiment.runtime,base.experiment.runtime,'Runtime source identity changed');
  const cp=run.trial?.checkpoint, t=cp?.final_transition;
  const success=Boolean(cp && !run.trial.preparation_failure && !cp.recording.error &&
    !cp.recording.runtime.failure && !t.terminated && t.truncated);
  return {name,journal:{path:run.path,sha256:run.sha256},
    cad_sha256:s.scene.robot.source.cad_sha256,completed_successfully:success,
    preparation_failure:run.trial?.preparation_failure??null,
    error:cp?.recording.error??null,physics_failure:cp?.recording.runtime.failure??null,
    time_s:t?.time_s??null,termination_reasons:t?.termination_reasons??[],
    net_speed_m_s:success?t.speed.net_speed_m_s:null,
    net_distance_m:success?t.speed.net_distance_m:null,
    relative_net_speed_change:success?t.speed.net_speed_m_s/base.trial.checkpoint.final_transition.speed.net_speed_m_s-1:null,
    endpoint_power_network:cp?.frame.power??null};
});
const report={scope:'Matched original gait over two seconds. Only declared CAD actuator/power profiles and power selection differ. Hypothetical scenarios; no sustained robustness or hardware calibration claim.',
  invariants_checked:['runtime source identity','mechanical definition','world/scene','gait/actions','controller policy','seed','task','horizon/numerical settings'],
  caveat:'Battery case also changes source voltage according to its initial SOC; its speed difference cannot be attributed solely to resistance.',cases};
fs.writeFileSync(`${root}/robustness/comparison.json`,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify(cases.map(({name,completed_successfully,net_speed_m_s,relative_net_speed_change})=>({name,completed_successfully,net_speed_m_s,relative_net_speed_change})),null,2));
