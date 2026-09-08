import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', out = 'runs/full-robot/learning/sdirk-physical-guess';
assert(!existsSync(out), 'physical-guess output already exists'); mkdirSync(out, {recursive: true});
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const prior = read(`${root}/sdirk-steering-plan.json`), status = read(`${root}/sdirk-steering-status.json`);
const cases = ['student-be-20ms', 'teacher-sdirk2-20ms', 'student-sdirk2-20ms'].map(name => {
  const base = prior.cases.find(c => c.name === name), result = status.cases.find(c => c.name === name);
  assert(base && result);
  for (const s of result.sources) assert.equal(source(s.path).sha256, s.sha256, s.path);
  return {...base, previous_capture: result.sources.find(s => s.path.endsWith('.native.json'))};
});
const plan = {version: 1, cases,
  sources: [`${root}/SDIRK2-PHYSICAL-GUESS.md`, `${root}/sdirk-steering-plan.json`,
    `${root}/sdirk-steering-status.json`, 'target/release/examples/run_environment',
    'target/release/examples/evaluate_lift',
    'crates/sim-domain-robot/src/articulated/embedding/implicit.rs',
    'crates/sim-domain-robot/src/articulated/embedding/mechanical_advance.rs',
    'crates/sim-domain-robot/tests/embedded_step.rs', 'crates/sim-domain-robot/tests/embedding.rs',
    'runs/sdirk-physical-guess-tests.log', 'runs/sdirk-physical-guess-build.log', import.meta.filename,
    ...cases.flatMap(c => [c.scene, c.config, c.task, c.actions, c.previous_capture.path])].map(source),
  scope: 'Same authored 24-second steering recipes, seed 0, CAD and task budgets. Experimental mechanical SDIRK2 now starts the second solve from the first physical stage, matching the vector primitive. Default backward Euler must preserve all previous physics and task transitions exactly. No browser or timestep-accuracy qualification.'};
writeFileSync(`${out}/plan.json`, JSON.stringify(plan, null, 2) + '\n');
writeFileSync(`${root}/sdirk-physical-guess-plan.json`, JSON.stringify(plan, null, 2) + '\n');
