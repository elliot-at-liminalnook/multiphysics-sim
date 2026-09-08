import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-turn-support';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const previous = read(`${root}/steering-plan.json`), cases = [], inputs = new Set(); mkdirSync(output, {recursive: true});
for (const shift of [-.024, -.027]) for (const oldName of ['student-turn-20ms', 'teacher-turn-5ms']) {
  const old = previous.cases.find(c => c.name === oldName), config = read(old.config);
  config.policy.step_reference.sequence.command_postures.find(p => p.forward_speed_m_s === 0).support_offsets_m[1][0] = shift;
  const name = `${oldName}-support-${Math.round(-shift * 1000)}mm`, paths = {};
  for (const [kind, value] of Object.entries({scene: read(old.scene), config, actions: read(old.actions)})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); inputs.add(old[kind]);
  }
  cases.push({...old, ...paths, name, neutral_front_support_x_m: shift});
}
const plan = {version: 1, cases, sources: [...inputs, `${root}/steering-plan.json`, `${root}/TURN-SUPPORT-PLAN.md`, `${root}/prepare_turn_support.mjs`,
  'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Four derived turning weight-shift candidates; unchanged speed envelope, physical model and acceptance budgets.'};
write(`${output}/plan.json`, plan); write(`${root}/turn-support-plan.json`, plan); console.log(output);
