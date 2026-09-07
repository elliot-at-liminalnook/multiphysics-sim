// Preserve the posture/timing/recovery study, including failed solves and references.
import {readFileSync,writeFileSync,readdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/step-margin', source='examples/full-robot/step-margin';
const read=p=>JSON.parse(readFileSync(p));
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const policy=read('examples/full-robot/student-robustness/policy.json');
const cases=[];
for(const file of readdirSync(root).filter(f=>f.endsWith('.native.json')).sort()) {
  const name=file.replace('.native.json',''), path=`${root}/${file}`, capture=read(path);
  const configPath=`${source}/${name==='sustained'?'config':name+'.config'}.json`;
  const config=read(configPath), sequence=config.policy.step_reference.sequence;
  assert.deepEqual(config.policy.neural_residual,policy,'study must not silently change network weights');
  const actions=/sustained|minute/.test(name)?'examples/full-robot/browser-residual-policy/sustained.actions.json':
    /reverse|fresh-x|margin-x|retry-x/.test(name)?'examples/full-robot/neural-teacher/heldout.actions.json':'examples/full-robot/neural-teacher/train.actions.json';
  const declaredActions=read(actions);
  assert(capture.recording.config,'native capture must contain the embedded runtime recipe');
  for(const event of capture.recording.input_events??[]) {
    const at=event.at_step*config.step_s/capture.task.period_s;
    assert(Math.abs(at-Math.round(at))<1e-7,'input event must align with controller sampling');
    assert.deepEqual(event.values,declaredActions[Math.round(at)],'action provenance mismatch: '+name);
  }
  const acceptancePath=`${root}/${name}-acceptance/summary.json`;
  const acceptance=existsSync(acceptancePath)?read(acceptancePath):null;
  if(capture.completed&&!capture.error)assert(acceptance,'completed case needs independent acceptance: '+name);
  const final=capture.transitions.at(-1), outcomes=capture.transitions.flatMap(t=>t.walking?.outcome?[t.walking.outcome]:[]);
  cases.push({name,config:configPath,config_sha256:hash(configPath),actions,actions_sha256:hash(actions),
    lift_m:sequence.lift_m,support_offsets_m:sequence.support_offsets_m,command_postures:sequence.command_postures,
    phase_durations_s:sequence.phase_durations_s,
    maximum_newton_iterations:config.implicit?.newton?.max_iterations,
    mechanical_subdivision:config.mechanical_subdivision??null,
    step_s:config.step_s,requested_steps:config.steps,world_loads:config.world_loads??null,
    completed:capture.completed,error:capture.error,time_s:final.time_s,
    qualified:final.walking?.qualified_steps,failed:final.walking?.failed_steps,
    final_body_error_m:final.walking?Math.hypot(...final.walking.body_error_world_m):null,
    passed:acceptance?.passed??false,acceptance,walking_outcomes:outcomes,capture_sha256:hash(path)});
}
const report={version:1,scope:'Planner posture, timing, and opt-in implicit-step recovery study using unchanged student weights, task gates, CAD robot and force laws. Partial episodes are failures, regardless of qualified swings before interruption. Fresh push cases become development data once used to choose later candidates. Recovery is not timestep accuracy control.',
  sources:['examples/full-robot/student-distillation/scene.json','examples/full-robot/walking-objective/task.json',
    'examples/full-robot/student-robustness/policy.json',`${source}/validation-plan.json`,`${source}/validation-wave2.json`,
    'crates/sim-runtime/src/step_reference.rs','crates/sim-runtime/src/embedded.rs',
    'crates/sim-domain-robot/src/articulated/embedding/mechanical_advance.rs'].map(path=>({path,sha256:hash(path)})),cases};
writeFileSync(`${source}/study-status.json`,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify(cases.map(c=>({name:c.name,completed:c.completed,passed:c.passed,qualified:c.qualified,failed:c.failed}))));
