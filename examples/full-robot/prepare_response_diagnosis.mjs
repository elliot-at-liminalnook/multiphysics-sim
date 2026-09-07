// Freeze accepted controller outputs for a controlled timestep comparison.
// The production Rhai adapter and Rust dynamics still execute every interval.
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [capturePath = 'runs/full-robot/learning/speed-envelope/1.5x-stop-feedback-1.native.json', output = 'runs/full-robot/learning/response-diagnosis'] = process.argv.slice(2);
const read = p => JSON.parse(readFileSync(p));
const source = read(capturePath);
assert(source.completed && !source.error);
const period = source.task.period_s;
assert.equal(source.frames.length, source.transitions.length);
const names = Object.keys(source.frames[1].policy.targets);
const rows = source.frames.slice(1).map((f, i) => {
  assert(Math.abs(f.time_s - (i + 1) * period) < 1e-9);
  assert(Math.abs(f.policy.time_s - i * period) < 1e-9, 'capture must contain each sampled target');
  assert.deepEqual(Object.keys(f.policy.targets), names);
  return names.map(n => f.policy.targets[n]);
});
const duration = rows.length * period;
const scene = structuredClone(source.recording.scene);
scene.controller.parameters = {target_names: names, target_rows: rows, period_s: period};
scene.controller.sources = {entry: 'frozen-targets.rhai', files: {
  'frozen-targets.rhai': 'fn control(t,s,c,state) { let p=parameters(); let i=((t / p.period_s)+0.00000001).to_int(); if i<0 || i>=p.target_rows.len() { throw "frozen target time outside capture"; } for j in 0..p.target_names.len() { c[p.target_names[j]]=p.target_rows[i][j]; } #{commands:c,state:state} }',
}};
mkdirSync(output, {recursive: true});
const write = (name, data) => writeFileSync(`${output}/${name}.json`, JSON.stringify(data) + '\n');
write('frozen.scene', scene);
write('task', source.task);
write('actions', source.frames.slice(1).map(f => f.policy_inputs));
for (const [label, step] of [['20ms', .02], ['10ms', .01], ['5ms', .005]]) {
  const config = structuredClone(source.recording.config);
  config.step_s = step;
  config.steps = Math.round(duration / step);
  if (step !== source.recording.config.step_s) config.implicit.newton.max_iterations = 80;
  write(`closed-${label}.config`, config);
  write(`frozen-${label}.config`, config);
}
const damped = structuredClone(source.recording.scene);
damped.options.floor_dissipation_s_m = 100;
write('dissipation-100.scene', damped);
const compliant = structuredClone(damped);
compliant.robot.world.floor_stiffness = 20000;
write('compliant.scene', compliant);
const audit = read(`${output}/closed-5ms.config.json`);
audit.implicit.newton_audit_window_s = [16.895, 16.915];
write('audit-5ms.config', audit);
write('manifest', {
  version: 1, source: {path: capturePath, sha256: createHash('sha256').update(readFileSync(capturePath)).digest('hex')},
  generator_sha256: createHash('sha256').update(readFileSync(import.meta.filename)).digest('hex'),
  period_s: period, duration_s: duration,
  variants: {
    frozen: 'Identical sampled motor targets from the source capture; actual dynamics remain live. Verify exact 20 ms reproduction before interpreting the 5 ms comparison. Online geometry guards and observation calculations remain enabled.',
    dissipation_100: 'Closed-loop source controller; only floor dissipation overridden to 100 s/m. Historical value is 0.2 s/m for this scene. This changes contact physics, not numerical tolerance.',
    compliant: 'Same explicit 100 s/m override plus environment floor spring 20000 N/m instead of 200000 N/m per sampled contact. Not a calibrated robot or floor property.',
  },
});
console.log(output);
