import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', out = 'runs/full-robot/learning/settled-stance';
assert(!existsSync(out), 'settled-stance output already exists'); mkdirSync(out, {recursive: true});
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const oldPlan = read(`${root}/transition-support-plan.json`), oldStatus = read(`${root}/transition-support-status.json`);
const base = oldPlan.cases.find(c => c.name === 'teacher-forward-support-24mm'); assert(base);
const previous = oldStatus.cases.find(c => c.name === base.name); assert(previous);
previous.sources.forEach(s => assert.equal(source(s.path).sha256, s.sha256, s.path));
const scriptPath = `${root}/settled-stance-teacher.rhai`, script = readFileSync(scriptPath, 'utf8');
const cases = [['reference', 1, .75], ['point-release', 0, .75], ['higher-body-gain', 1, 3.75], ['combined', 0, 3.75]].map(([name, pointScale, increment]) => {
  const scene = read(base.scene); scene.controller.sources = {entry: 'settled-stance-teacher.rhai', files: {'settled-stance-teacher.rhai': script}};
  scene.controller.parameters.settled_point_gain_scale = pointScale;
  scene.controller.parameters.settled_gain_increment = increment;
  const path = `${out}/${name}.scene.json`; writeFileSync(path, JSON.stringify(scene) + '\n');
  return {...base, name, scene: path, settled_point_gain_scale: pointScale,
    maximum_settled_body_gain: .25 + .5 + increment, seed: 0};
});
const plan = {version: 1, cases, previous_capture: previous.sources.find(s => s.path.endsWith('.native.json')),
  sources: [`${root}/SETTLED-STANCE-PLAN.md`, `${root}/transition-support-plan.json`,
    `${root}/transition-support-status.json`, scriptPath, import.meta.filename,
    'target/release/examples/run_environment', 'target/release/examples/evaluate_lift',
    ...cases.flatMap(c => [c.scene, c.config, c.actions, c.task]),
    previous.sources.find(s => s.path.endsWith('.native.json')).path].map(source),
  scope: 'Predeclared 2x2 settled stance feedback ablation. Same 32-second revealed development sequence, fine physics, CAD and task gates. Require original reference identity and unchanged pre-stop frames. No new held-out or browser qualification.'};
for (const path of [`${out}/plan.json`, `${root}/settled-stance-plan.json`]) writeFileSync(path, JSON.stringify(plan, null, 2) + '\n');
