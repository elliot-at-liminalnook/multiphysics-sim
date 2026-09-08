import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-cycle';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const previous = read(`${root}/plan.json`), cases = [], inputs = new Set();
mkdirSync(output, {recursive: true});
for (const c of previous.cases.filter(c => c.speed > .0025 && c.stance === 'all' && c.support === 'derived')) {
  const scene = read(c.scene), config = read(c.config), actions = read(c.actions), seq = config.policy.step_reference.sequence;
  const period = seq.phase_durations_s.reduce((s, v) => s + v, 0);
  for (const p of [seq, ...seq.command_postures.filter(p => p.forward_speed_m_s >= 0)]) p.support_offsets_m[3][0] -= 3 * period * (c.speed - .0025);
  const name = `cycle-${c.speed * 1000}-${c.step_s * 1000}ms`, paths = {};
  for (const [kind, value] of Object.entries({scene, config, actions})) { paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); inputs.add(c[kind]); }
  cases.push({...c, name, ...paths, rear_support_x_m: seq.support_offsets_m[3][0]});
}
const plan = {version: 1, cases, sources: [...inputs, `${root}/plan.json`, `${root}/prepare_cycle.mjs`, `${root}/CYCLE-PLAN.md`].map(path =>
  ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Derived rear support alignment after the all-foot/front-support whole-swing experiment. Same CAD physics, network, bounds and predeclared acceptance gates. Development only.'};
write(`${output}/plan.json`, plan); write(`${root}/cycle-plan.json`, plan); console.log(output);
