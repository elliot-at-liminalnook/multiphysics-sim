import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-minute-refinement';
const read = p => JSON.parse(readFileSync(p)), write = (p,v) => writeFileSync(p,JSON.stringify(v)+'\n');
mkdirSync(output,{recursive:true});
const actionPath = `${output}/tested.actions.json`;
execFileSync(process.execPath,['web/leaderboard/materialize_actions.mjs','settled-teacher-minute',actionPath]);
const cases = [];
for (const step of [.00125,.000625]) {
  const name = `combined-minute-${step*1000}ms`, config = read(`${root}/combined-minute.config.json`), paths = {};
  config.step_s=step;config.steps=Math.round(60/step);config.report_every=Math.round(.02/step);
  for (const [kind,value] of Object.entries({config,scene:read(`${root}/combined-minute.scene.json`),actions:read(actionPath)})) {
    paths[kind]=`${output}/${name}.${kind}.json`;write(paths[kind],value);
  }
  cases.push({name,...paths,task:'examples/full-robot/heading-task/task.json',duration_s:60,step_s:step});
}
const plan={version:1,cases,sources:[`${root}/MINUTE-REFINEMENT-PLAN.md`,`${root}/prepare_minute_refinement.mjs`,`${root}/combined-minute.scene.json`,
  `${root}/combined-minute.config.json`,'web/leaderboard/evaluations.json','web/leaderboard/materialize_actions.mjs',
  'crates/sim-solve/src/lib.rs','crates/sim-solve/src/profile.rs','crates/sim-domain-control/src/stepping.rs'].map(path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope:'Repeated 1.25 ms and new 0.625 ms minute-long combined teacher; identical physical model, controller, inputs and accuracy gates; no secant updates.'};
write(`${output}/plan.json`,plan);write(`${root}/minute-refinement-plan.json`,plan);
