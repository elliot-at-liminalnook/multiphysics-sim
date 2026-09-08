import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', out = 'runs/full-robot/learning/settled-integral';
assert(!existsSync(out), 'settled-integral output already exists'); mkdirSync(out, {recursive: true});
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const oldPlan = read(`${root}/settled-stance-plan.json`), oldStatus = read(`${root}/settled-stance-status.json`);
const base = oldPlan.cases.find(c => c.name === 'point-release'); assert(base);
const previous = oldStatus.cases.find(c => c.name === base.name); assert(previous);
previous.sources.forEach(s => assert.equal(source(s.path).sha256, s.sha256, s.path));
const previousCapture = previous.sources.find(s => s.path.endsWith('.native.json'));
const captured = read(previousCapture.path);
assert.equal(captured.contract.period_s, .02); assert.equal(captured.metadata.policy_contract.period_s, .02);
const scriptPath = `${root}/settled-integral-teacher.rhai`, script = readFileSync(scriptPath, 'utf8');
const cases = [0, .5, 1].map(gain => {
  const scene = read(base.scene), name = `integral-${gain}`;
  scene.controller.sources = {entry: 'settled-integral-teacher.rhai', files: {'settled-integral-teacher.rhai': script}};
  scene.controller.parameters.stance_integrator = {period_s: .02, integral_gain_per_s: gain,
    leak_rate_per_s: 0, maximum_bias_rad: .04, maximum_rate_rad_s: .01};
  assert.equal(scene.controller.parameters.settled_point_gain_scale, 0);
  assert.equal(scene.controller.parameters.settled_gain_increment, .75);
  const path = `${out}/${name}.scene.json`; writeFileSync(path, JSON.stringify(scene) + '\n');
  return {...base, name, scene: path, integral_gain_per_s: gain, seed: 0};
});
const plan = {version: 1, cases, previous_capture: previousCapture,
  sources: [`${root}/SETTLED-INTEGRAL-PLAN.md`, `${root}/settled-stance-plan.json`,
    `${root}/settled-stance-status.json`, scriptPath, import.meta.filename,
    'crates/sim-domain-control/src/angle_integral.rs', 'crates/sim-domain-control/src/elements.rs',
    'crates/sim-script/src/lib.rs', 'crates/sim-script/Cargo.toml',
    'crates/sim-domain-control/tests/angle_integral.rs', 'crates/sim-script/tests/angle_integral.rs',
    'runs/angle-integral-final-tests.log', 'runs/angle-integral-rhai-tests.log', 'runs/angle-integral-native-build.log',
    'target/release/examples/run_environment', 'target/release/examples/evaluate_lift',
    ...cases.flatMap(c => [c.scene, c.config, c.actions, c.task]), previousCapture.path].map(source),
  scope: 'Predeclared bounded integral gains 0/0.5/1 per second at stable proportional gain. Same revealed 32-second development inputs, fine physics, CAD and task budgets. Require exact gain-zero reference and pre-stop frames. Bias is a rate-limited motor target correction, never a pose or force override. No held-out or browser qualification.'};
for (const path of [`${out}/plan.json`, `${root}/settled-integral-plan.json`]) writeFileSync(path, JSON.stringify(plan, null, 2) + '\n');
