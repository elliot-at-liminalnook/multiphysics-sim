import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-feedback-teacher';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const previous = read(`${root}/teacher-plan.json`), cases = [], inputs = new Set();
mkdirSync(output, {recursive: true});
for (const old of previous.cases) {
  const name = `feedback-${old.name}`, config = read(old.config);
  config.policy.task_observations.floor_forces = true;
  const paths = {};
  for (const [kind, value] of Object.entries({scene: read(old.scene), config, actions: read(old.actions)})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); inputs.add(old[kind]);
  }
  cases.push({...old, ...paths, name});
}
const plan = {version: 1, cases, sources: [...inputs, `${root}/teacher-plan.json`, `${root}/FEEDBACK-TEACHER-PLAN.md`, `${root}/prepare_feedback_teacher.mjs`,
  'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Two privileged teacher diagnosis cases with the required force observations enabled; same physical model and gates.'};
write(`${output}/plan.json`, plan); write(`${root}/feedback-teacher-plan.json`, plan); console.log(output);
