import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, mkdir, readFile, writeFile, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {eligible, rankEntries, requiredGates, validateEntry} from '../viewer/leaderboard-model.mjs';
import {packageLeaderboard} from '../leaderboard/package.mjs';
const data = JSON.parse(await readFile(new URL('../leaderboard/evaluations.json', import.meta.url)));
test('failed or absent evidence cannot create a speed rank', () => {
  for (const entry of data.entries) { validateEntry(entry); assert(!eligible(entry)); }
  assert.equal(rankEntries(data.entries).size, 0);
  const good = structuredClone(data.entries[0]);
  good.gates = Object.fromEntries(requiredGates.map(k => [k, {status: 'pass', detail: 'Synthetic unit-test evidence'}]));
  good.metrics.simulated_s = 60; good.metrics.speed_window_s = 40;
  assert(eligible(good));
  for (const key of requiredGates) for (const status of ['fail', 'missing', undefined]) {
    const bad = structuredClone(good); bad.gates[key].status = status;
    assert(!eligible(bad), key); assert.equal(rankEntries([bad]).size, 0);
  }
  for (const speed of [null, NaN, Infinity, -1]) assert(!eligible({...good, metrics: {...good.metrics, sustained_speed_m_s: speed}}));
  assert(!eligible({...good, metrics: {...good.metrics, simulated_s: 24}}));
  assert(!eligible({...good, metrics: {...good.metrics, speed_window_s: 10}}));
  const faster = {...good, id: 'faster', metrics: {...good.metrics, sustained_speed_m_s: .01}};
  const different = {...faster, id: 'different', comparison_group: 'other-model'};
  assert.deepEqual([...rankEntries([good, faster, different])], [['faster', 1], [good.id, 2], ['different', 1]]);
});
test('packaging verifies exact recipes and rejects stale evidence before publishing a catalog', async () => {
  const root = resolve(import.meta.dirname, '../..'), temp = await mkdtemp(join(tmpdir(), 'robot-leaderboard-'));
  try {
    await mkdir(join(temp, 'data'));
    const catalog = {presets: []}, manifest = {inputs: {}, presets: []};
    await packageLeaderboard(root, temp, catalog, manifest, false);
    assert.equal(catalog.presets.length, data.entries.length);
    for (const [i, preset] of catalog.presets.entries()) {
      const recipe = JSON.parse(await readFile(join(temp, preset.path)));
      assert.deepEqual(recipe.config, JSON.parse(await readFile(join(root, data.entries[i].load.config.path))));
      assert.equal(recipe.seed, data.entries[i].load.seed);
    }
    const changedRoot = join(temp, 'changed'); await mkdir(join(changedRoot, 'web/leaderboard'), {recursive: true});
    const edited = structuredClone(data); edited.entries[0].load.scene.sha256 = '0'.repeat(64);
    await writeFile(join(changedRoot, 'web/leaderboard/evaluations.json'), JSON.stringify(edited));
    const source = edited.entries[0].load.scene.path;
    const generator = data.generator.path;
    await mkdir(join(changedRoot, generator, '..'), {recursive: true});
    await writeFile(join(changedRoot, generator), await readFile(join(root, generator)));
    await mkdir(join(changedRoot, source, '..'), {recursive: true});
    await writeFile(join(changedRoot, source), await readFile(join(root, source)));
    await assert.rejects(packageLeaderboard(changedRoot, temp, {presets: []}, {inputs: {}, presets: []}, false), /Stale leaderboard evidence or recipe/);
  } finally { await rm(temp, {recursive: true, force: true}); }
});
