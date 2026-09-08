// Reconstruct exact tested 50 Hz inputs from a hash-bound leaderboard recipe.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {validateEntry} from '../viewer/leaderboard-model.mjs';
const [id, output] = process.argv.slice(2);
assert(id && output, 'usage: materialize_actions.mjs entry-id output.json');
const entry = JSON.parse(readFileSync('web/leaderboard/evaluations.json')).entries.find(e => e.id === id);
assert(entry, `Unknown tested controller ${id}`); validateEntry(entry);
const readSource = source => {
  const bytes = readFileSync(source.path);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), source.sha256, source.path);
  return JSON.parse(bytes);
};
const scene = readSource(entry.load.scene), config = readSource(entry.load.config), task = readSource(entry.load.task);
const stride = Math.round(task.period_s / config.step_s);
assert(Math.abs(stride * config.step_s - task.period_s) < 1e-12);
let held = scene.controller.inputs.map(c => c.initial), next = 0;
const actions = [], events = entry.replay.input_events;
for (let at = 0; at < entry.replay.completed_steps; at += stride) {
  if (events[next]?.at_step === at) held = events[next++].values;
  actions.push([...held]);
}
assert.equal(next, events.length);
writeFileSync(output, JSON.stringify(actions) + '\n');
