import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';

const directory = import.meta.dirname, root = path.dirname(directory);
const read = relative => JSON.parse(fs.readFileSync(path.join(root, relative + '.json')));
const baseline = 'mechanical-predictor/numeric/candidate';
const prefix = name => `solver-tolerance-refinement/${name}/candidate`;
const names = ['tight-6400', 'tighter-6400', 'default-12800', 'tighter-12800'];
const normalize = config => {
  const copy = structuredClone(config);
  delete copy.step_s; delete copy.steps; delete copy.report_every;
  delete copy.implicit.newton.absolute_tolerance;
  delete copy.implicit.newton.relative_tolerance;
  return copy;
};
const baselineConfig = read('mechanical-predictor/numeric/config');
const baselineIdentity = read(baseline + '.binary').sha256;
for (const name of names) {
  const recipe = `solver-tolerance-refinement/${name}`;
  assert.deepEqual(normalize(read(recipe + '/config')), normalize(baselineConfig));
  for (const field of ['scene', 'task', 'actions']) {
    assert.deepEqual(read(recipe + '/' + field), read('mechanical-predictor/numeric/' + field));
  }
  assert.equal(read(prefix(name) + '.binary').sha256, baselineIdentity);
}
const pairs = [
  ['default-vs-tight', baseline, prefix('tight-6400')],
  ['tight-vs-tighter', prefix('tight-6400'), prefix('tighter-6400')],
  ['default-vs-tighter-6400', baseline, prefix('tighter-6400')],
  ['default-vs-tighter-12800', prefix('default-12800'), prefix('tighter-12800')],
  ['default-timestep', baseline, prefix('default-12800')],
  ['tighter-timestep', prefix('tighter-6400'), prefix('tighter-12800')],
];
const comparisons = [];
for (const [name, candidate, reference] of pairs) {
  const output = `solver-tolerance-refinement/${name}`;
  const result = spawnSync(process.execPath, [path.join(root, 'compare-refinement.mjs'),
    candidate, reference, output], {encoding: 'utf8'});
  assert.equal(result.status, 0, result.stderr);
  const a = read(candidate + '.native'), b = read(reference + '.native');
  const comparison = read(output).results[0];
  comparison.saved_servo_states_exact = a.frames.every((frame, i) =>
    JSON.stringify(frame.servo_states) === JSON.stringify(b.frames[i].servo_states));
  comparison.saved_servo_commands_exact = a.frames.every((frame, i) =>
    JSON.stringify(frame.servo_commands) === JSON.stringify(b.frames[i].servo_commands));
  comparisons.push({name, ...comparison});
}
const summary = {
  scope: 'Three-second fixed-input numerical sensitivity only. Physical inputs, controller clocks and binary identity verified equal. Only solver tolerances and integration timestep vary. No continuous-time or hardware reference is established. Saved FPGA comparisons cover 50 Hz snapshots, not every 400 Hz sample.',
  binary_sha256: baselineIdentity,
  comparisons,
};
fs.writeFileSync(path.join(directory, 'comparison.json'), JSON.stringify(summary, null, 2) + '\n');
for (const comparison of comparisons) console.log(comparison.name, JSON.stringify({
  physical_gates: comparison.passes_existing_physical_gates,
  max_error: comparison.max_error,
  saved_servo_states_exact: comparison.saved_servo_states_exact,
}));
