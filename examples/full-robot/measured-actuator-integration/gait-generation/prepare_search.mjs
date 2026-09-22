// Configuration preparation only. Search and all simulation run in shared Rust.
// Run from repo root: node <this-file> [seconds] [fresh-output] [values-json|-] [step-divisor] [base-spec-json]
import fs from 'node:fs';
import crypto from 'node:crypto';
const root = 'examples/full-robot/measured-actuator-integration';
const out = process.argv[3] ?? `${root}/gait-generation`;
for (const name of ['pilot.spec.json', 'pilot.settings.json', 'preparation.json'])
  if (fs.existsSync(`${out}/${name}`)) throw Error(`Refusing to replace ${out}/${name}; use a fresh output directory.`);
const paths = {
  historical: 'examples/full-robot/trusted-baseline/fastest573.input.json',
  scene: `${root}/controller-integration/provisional.scene.json`,
  config: `${root}/controller-integration/provisional.config.json`,
};
const read = p => JSON.parse(fs.readFileSync(p, 'utf8'));
const original = read(paths.historical);
const baseSpec = process.argv[6] ? read(process.argv[6]) : null;
if(baseSpec){
  paths.base_spec=process.argv[6];delete paths.scene;delete paths.config;
  if(baseSpec.task.period_s!==original.task.period_s ||
     JSON.stringify(baseSpec.scene.controller.inputs.map(i=>[i.name,i.kind]))!==JSON.stringify(original.runtime.scene.controller.inputs.map(i=>[i.name,i.kind])))
    throw Error('Base experiment must preserve the historical command contract and cadence.');
  for(let i=0;i<baseSpec.source_actions.length;i++)
    if(JSON.stringify(baseSpec.source_actions[i])!==JSON.stringify(original.runtime.input_events[i]?.values))
      throw Error('Base experiment source command prefix differs from historical schedule.');
}
const scene = baseSpec ? structuredClone(baseSpec.scene) : read(paths.scene);
const config = baseSpec ? structuredClone(baseSpec.config) : read(paths.config);
const horizon = Number(process.argv[2] ?? 2);
const period = original.task.period_s;
const count = Math.round(horizon / period);
if (!(horizon > 0 && horizon <= 300) || Math.abs(count * period - horizon) > 1e-10)
  throw Error('Horizon must be positive, at most 300 s, and on the task grid.');
config.steps = Math.round(horizon / config.step_s);
const stride = Math.round(period / original.runtime.config.step_s);
const events = original.runtime.input_events;
const actions = Array.from({length: count}, (_, i) => {
  const e = events[i];
  if (!e || e.at_step !== i * stride) throw Error('Historical command schedule differs from expected grid.');
  return e.values;
});
const divisor = Number(process.argv[5] ?? 1);
if (![1, 2].includes(divisor)) throw Error('Supported timestep divisors are 1 and 2.');
config.step_s /= divisor;
config.steps *= divisor;
config.report_every *= divisor;
const constant = value => ({source: 'constant', value});
const parameter = name => ({source: 'parameter', name});
const command = (input, kind, scale, offset) => ({input, kind, scale, center: constant(0), offset});
const baselineGain = actions[0][scene.controller.inputs.findIndex(i => i.name === 'command.tracking_gain')];
const spec = {
  version: 1, scene, config, task: original.task,
  parameterization: {
    version: 1,
    space: {parameters: [
      {name: 'pace_scale', kind: 'Dimensionless', bounds: [0.25, 1]},
      {name: 'tracking_gain', kind: 'Dimensionless', bounds: [-1, 1]},
    ]},
    trajectories: [],
    commands: [
      command('command.forward_speed', 'LinearVelocity', parameter('pace_scale'), constant(0)),
      command('command.yaw_rate', 'AngularVelocity', parameter('pace_scale'), constant(0)),
      command('command.tracking_gain', 'Dimensionless', constant(0), parameter('tracking_gain')),
    ],
  },
  source_actions: actions,
  baseline: {pace_scale: 1, tracking_gain: baselineGain},
  seed: original.runtime.seed, objective: 'net_speed',
};
if(baseSpec){
  for(const key of ['parameterization','task','baseline','seed','objective'])
    spec[key]=structuredClone(baseSpec[key]);
}
if (process.argv[4] && process.argv[4] !== '-') {
  paths.baseline_values = process.argv[4];
  spec.baseline = read(paths.baseline_values);
}
fs.mkdirSync(out, {recursive: true});
const write = (name, value) => fs.writeFileSync(`${out}/${name}`, JSON.stringify(value) + '\n');
write('pilot.spec.json', spec);
write('pilot.settings.json', {seed: 2301, initial_design: 3, acquisition_starts: 4, maximum_training_rows: 64});
write('preparation.json', {
  version: 1, horizon_s: horizon, action_intervals: count, step_s: config.step_s,
  preparation_script_sha256: crypto.createHash('sha256').update(fs.readFileSync(new URL(import.meta.url))).digest('hex'),
  source_sha256: Object.fromEntries(Object.entries(paths).map(([name,p]) => [name, {
    path: p, sha256: crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex'),
  }])),
  scope: 'Provisional detailed motors and CAD FPGA controller; unchanged imposed supplies. Two-coordinate pilot tunes pace and tracking feedback, preserving trajectory shape and historical packet refresh schedule. Not a new calibrated profile or a sustained-speed qualification.',
  baseline: spec.baseline,
  search_bounds: 'Authored optimization ranges, not measured physical limits. Pace search is at or below the historical command; yaw scales with forward speed to retain requested curvature.',
});
console.log(JSON.stringify({horizon_s: horizon, action_intervals: count, baseline: spec.baseline}));
