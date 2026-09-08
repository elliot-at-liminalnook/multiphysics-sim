import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-steering';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const neutralPath = 'examples/full-robot/student-speed/2x-support-18-9.config.json';
const neutral = read(neutralPath).policy.step_reference.sequence.command_postures.find(p => p.forward_speed_m_s === 0);
const cases = [], inputs = new Set([neutralPath]); mkdirSync(output, {recursive: true});
for (const [label, planPath, oldName, mixed] of [
  ['student-turn', `${root}/finish-plan.json`, 'finish-0.85-20ms', true],
  ['teacher-turn', `${root}/feedback-teacher-plan.json`, 'feedback-teacher-5ms', true],
  ['teacher-turn', `${root}/refinement-plan.json`, 'teacher-2.5ms', true],
  ['student-forward', `${root}/finish-plan.json`, 'finish-0.85-20ms', false],
]) {
  inputs.add(planPath); const old = read(planPath).cases.find(c => c.name === oldName), config = read(old.config), scene = read(old.scene);
  const sequence = config.policy.step_reference.sequence;
  const forward = structuredClone(sequence.command_postures.find(p => p.forward_speed_m_s === 0)); forward.forward_speed_m_s = .00375;
  const reverse = structuredClone(sequence.command_postures[0]); reverse.forward_speed_m_s = -.00125;
  sequence.command_postures = [reverse, structuredClone(neutral), forward];
  scene.controller.inputs.find(i => i.name === 'command.forward_speed').lower = -.00125;
  const name = `${label}-${config.step_s * 1000}ms`, paths = {};
  const actions = mixed ? Array.from({length: 1200}, (_, i) => scene.controller.inputs.map(c => {
    const t = i * .02;
    if (c.name === 'command.forward_speed') return t < 8.4 ? .00375 : t >= 16.8 && t < 20 ? -.00125 : 0;
    if (c.name === 'command.yaw_rate') return t >= 8.4 && t < 16.8 ? .001 : 0;
    return c.initial;
  })) : read(old.actions);
  for (const [kind, value] of Object.entries({scene, config, actions})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); inputs.add(old[kind]);
  }
  cases.push({...old, ...paths, name, scenario: mixed ? 'forward-turn-reverse-stop' : 'forward-preservation'});
}
const plan = {version: 1, cases, sources: [...inputs, `${root}/STEERING-PLAN.md`, `${root}/prepare_steering.mjs`,
  'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Four development cases separating fast forward from pure-turn posture, with a declared slower reverse envelope and unchanged physical gates.'};
write(`${output}/plan.json`, plan); write(`${root}/steering-plan.json`, plan); console.log(output);
