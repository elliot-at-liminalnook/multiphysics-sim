import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', out = 'runs/full-robot/learning/settled-integral-regression';
assert(!existsSync(out), 'regression output already exists'); mkdirSync(out, {recursive: true});
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const selectedPlan = read(`${root}/settled-integral-numeric-plan.json`);
const selected = selectedPlan.cases.find(c => c.name === 'integral-1'); assert(selected);
const result = read(`${root}/settled-integral-numeric-status.json`).cases.find(c => c.name === selected.name);
assert(result.passed); result.sources.forEach(s => assert.equal(source(s.path).sha256, s.sha256, s.path));
const scene = read(selected.scene), config = read(selected.config);
const scenePath = `${root}/settled-integral-candidate.scene.json`, configPath = `${root}/settled-integral-candidate.config.json`;
for (const [path, value] of [[scenePath, scene], [configPath, config]]) writeFileSync(path, JSON.stringify(value) + '\n', {flag: 'wx'});
const old = read(`${root}/combined-plan.json`);
const schedule = [[0, 0, 0], [4, .00375, 0], [12, 0, 0], [16, -.00125, 0],
  [24, 0, 0], [28, .00375, .001], [36, 0, -.001], [42, 0, 0]];
const freshActions = Array.from({length: 2400}, (_, i) => {
  const command = schedule.findLast(([t]) => i * .02 >= t), values = scene.controller.inputs.map(c => c.initial);
  for (const [name, value] of [['command.forward_speed', command[1]], ['command.yaw_rate', command[2]]]) {
    const j = scene.controller.inputs.findIndex(c => c.name === name); assert(j >= 0);
    assert(value >= scene.controller.inputs[j].lower && value <= scene.controller.inputs[j].upper); values[j] = value;
  }
  return values;
});
const definitions = [
  ['minute-1.25ms', 60, .00125, 'combined-minute-1.25ms', 'development'],
  ['steering-1.25ms', 24, .00125, 'combined-turn-1.25ms', 'development'],
  ['minute-0.625ms', 60, .000625, 'combined-minute-1.25ms', 'refinement'],
  ['fresh-commands', 48, .00125, null, 'heldout'],
  ['fresh-pushes', 48, .00125, null, 'heldout'],
];
const sources = [`${root}/SETTLED-INTEGRAL-REGRESSION.md`, `${root}/settled-integral-summary.json`,
  `${root}/settled-integral-numeric-plan.json`, `${root}/combined-plan.json`, scenePath, configPath,
  'target/release/examples/run_environment', 'target/release/examples/evaluate_lift',
  'crates/sim-domain-control/src/angle_integral.rs', 'crates/sim-script/src/lib.rs',
  'runs/angle-integral-final-tests.log', 'runs/angle-integral-numeric-tests.log',
  'runs/angle-integral-numeric-build.log', import.meta.filename];
const cases = definitions.map(([name, duration, step, oldName, split]) => {
  const c = structuredClone(config); c.step_s = step; c.steps = Math.round(duration / step); c.report_every = Math.round(.02 / step);
  let actions;
  if (oldName) {
    const b = old.cases.find(c => c.name === oldName); assert(b);
    assert.deepEqual(read(b.scene).controller.inputs, scene.controller.inputs);
    actions = read(b.actions); sources.push(b.scene, b.actions);
  } else {
    actions = freshActions;
    c.world_loads = {...c.world_loads, maximum_force_n: 0, pulses: [],
      provenance: 'Fresh held-out command case, explicitly unforced. SETTLED-INTEGRAL-REGRESSION.md'};
    if (name === 'fresh-pushes') c.world_loads = {...c.world_loads, maximum_force_n: .75,
      provenance: 'Predeclared hypothetical held-out pushes; not a calibrated hardware distribution. SETTLED-INTEGRAL-REGRESSION.md',
      pulses: [{name: 'reverse-lateral', start_s: 19.2, duration_s: .24, force_world_n: [0, -.75, 0], moment_world_nm: [0, 0, 0]},
        {name: 'turn-forward', start_s: 33.1, duration_s: .2, force_world_n: [-.5, 0, 0], moment_world_nm: [0, 0, 0]}]};
  }
  const cfg = `${out}/${name}.config.json`, act = `${out}/${name}.actions.json`;
  writeFileSync(cfg, JSON.stringify(c) + '\n'); writeFileSync(act, JSON.stringify(actions) + '\n');
  sources.push(cfg, act, selected.task);
  return {name, scene: scenePath, config: cfg, actions: act, task: selected.task,
    duration_s: duration, step_s: step, seed: 0, split};
});
const plan = {version: 1, cases, heldout_schedule_s: schedule, intermediate_stop_check_times_s: [15.98, 27.98, 48],
  sources: [...new Set(sources)].map(source),
  scope: 'Frozen gain-1 integral teacher: sustained and steering development, minute timestep refinement, and two fresh held-out stop-to-motion cases. Original task and trajectory budgets. Explicit unforced/pushed held-out environments. No tuning from these outcomes or browser/terrain/hardware qualification.'};
for (const path of [`${out}/plan.json`, `${root}/settled-integral-regression-plan.json`]) writeFileSync(path, JSON.stringify(plan, null, 2) + '\n');
