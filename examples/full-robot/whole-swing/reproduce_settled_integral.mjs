// Reconstruct the frozen experiment from versioned recipes, without old runs/ inputs.
import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const output = process.argv[2];
assert(output && !existsSync(output), 'usage: reproduce_settled_integral.mjs fresh-output-directory');
const root = 'examples/full-robot/whole-swing';
const archivePath = `${root}/settled-integral-regression-recipes.json`;
const read = path => JSON.parse(readFileSync(path));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const archive = read(archivePath);
for (const s of [archive.scene, archive.task]) assert.equal(source(s.path).sha256, s.sha256, s.path);
const scene = read(archive.scene.path), task = read(archive.task.path);
mkdirSync(output, {recursive: true});
const cases = archive.cases.map(c => {
  const stride = Math.round(task.period_s / c.config.step_s);
  assert(Math.abs(stride * c.config.step_s - task.period_s) < 1e-12);
  assert.equal(c.config.steps % stride, 0);
  let held = scene.controller.inputs.map(c => c.initial), next = 0;
  const actions = [];
  for (let step = 0; step < c.config.steps; step += stride) {
    if (c.input_events[next]?.at_step === step) held = c.input_events[next++].values;
    actions.push([...held]);
  }
  assert.equal(next, c.input_events.length);
  assert.equal(createHash('sha256').update(JSON.stringify(actions)).digest('hex'), c.actions_json_sha256);
  const config = `${output}/${c.name}.config.json`, actionPath = `${output}/${c.name}.actions.json`;
  writeFileSync(config, JSON.stringify(c.config) + '\n');
  writeFileSync(actionPath, JSON.stringify(actions) + '\n');
  return {name: c.name, scene: archive.scene.path, config, actions: actionPath,
    task: archive.task.path, duration_s: c.config.steps * c.config.step_s,
    step_s: c.config.step_s, seed: c.seed, split: c.split};
});
const sources = [archivePath, archive.scene.path, archive.task.path, import.meta.filename,
  'target/release/examples/run_environment', 'target/release/examples/evaluate_lift',
  ...cases.flatMap(c => [c.config, c.actions])].map(source);
writeFileSync(`${output}/plan.json`, JSON.stringify({version: 1, cases, sources,
  intermediate_stop_check_times_s: archive.intermediate_stop_check_times_s,
  scope: 'Reproduction of the frozen integral teacher, including original held-out cases. These previously revealed cases are regression evidence for later changes, not new held-outs.'}, null, 2) + '\n');
console.log(`Prepared ${cases.length} frozen cases in ${output}`);
