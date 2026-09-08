import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing';
const out = 'runs/full-robot/learning/sdirk-tolerance-diagnosis';
assert(!existsSync(out), 'diagnosis output already exists'); mkdirSync(out, {recursive: true});
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const previous = read(`${root}/sdirk-steering-plan.json`);
const base = previous.cases.find(c => c.name === 'teacher-sdirk2-20ms'); assert(base);
const cases = ['authored', 'tight'].map(kind => {
  const config = read(base.config);
  assert.equal(config.implicit.newton.absolute_tolerance, 1e-5);
  assert.equal(config.implicit.newton.relative_tolerance, 1e-5);
  if (kind === 'tight') {
    config.implicit.newton.absolute_tolerance = 1e-8;
    config.implicit.newton.relative_tolerance = 1e-8;
  }
  const name = `teacher-20ms-${kind}`, configPath = `${out}/${name}.config.json`;
  writeFileSync(configPath, JSON.stringify(config) + '\n');
  return {...base, name, config: configPath, profile: `${out}/${name}.profile.json`};
});
const plan = {version: 1, cases,
  sources: [`${root}/SDIRK2-FAILURE-DIAGNOSIS.md`, `${root}/sdirk-steering-plan.json`,
    base.scene, base.config, base.actions, base.task, 'target/release/examples/run_environment',
    'target/release/examples/evaluate_lift', import.meta.filename].map(source),
  scope: 'Predeclared nonlinear tolerance diagnosis, not a new controller or a performance test. Same seed 0, CAD model and 24-second development inputs, with instrumented profiles. Only absolute/relative Newton tolerances differ.'};
writeFileSync(`${out}/plan.json`, JSON.stringify(plan, null, 2) + '\n');
writeFileSync(`${root}/sdirk-tolerance-plan.json`, JSON.stringify(plan, null, 2) + '\n');
