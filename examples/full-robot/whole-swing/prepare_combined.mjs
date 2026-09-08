import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-combined';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const standing = read(`${root}/settled-plan.json`).cases.find(c => c.step_s === .0025);
const turning = read(`${root}/turn-support-plan.json`).cases.find(c => c.name === 'teacher-turn-5ms-support-24mm');
const cases = []; mkdirSync(output, {recursive: true});
for (const mixed of [false, true]) for (const step of [.0025, .00125]) {
  const config = read(standing.config), scene = read(standing.scene), duration = mixed ? 24 : 60;
  config.policy.step_reference.sequence.command_postures = read(turning.config).policy.step_reference.sequence.command_postures;
  config.step_s = step; config.steps = Math.round(duration / step); config.report_every = Math.round(.02 / step);
  scene.controller.inputs.find(i => i.name === 'command.forward_speed').lower = -.00125;
  const name = `combined-${mixed ? 'turn' : 'minute'}-${step * 1000}ms`, paths = {};
  for (const [kind, value] of Object.entries({scene, config, actions: read(mixed ? turning.actions : standing.actions)})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value);
  }
  cases.push({...standing, ...paths, name, duration_s: duration, step_s: step, scenario: mixed ? 'forward-turn-reverse-stop' : 'minute-forward-stop'});
}
const plan = {version: 1, cases, sources: [`${root}/settled-plan.json`, `${root}/turn-support-plan.json`, standing.scene, standing.config,
  standing.actions, turning.config, turning.actions, `${root}/COMBINED-PLAN.md`, `${root}/prepare_combined.mjs`,
  'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Four combined-policy development cases at 2.5/1.25 ms; unchanged physical model and accuracy/task budgets.'};
write(`${output}/plan.json`, plan); write(`${root}/combined-plan.json`, plan); console.log(output);
