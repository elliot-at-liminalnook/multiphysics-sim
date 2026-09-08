import {readFile, writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {validateEntry} from '../viewer/leaderboard-model.mjs';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
export async function packageLeaderboard(root, output, catalog, manifest, fixtureOnly) {
  const path = 'web/leaderboard/evaluations.json', bytes = await readFile(join(root, path));
  const evaluations = JSON.parse(bytes); assert.equal(evaluations.version, 1);
  const generator = evaluations.generator;
  assert.equal(hash(await readFile(join(root, generator.path))), generator.sha256, 'Stale evaluation generator; regenerate the catalog');
  manifest.inputs[generator.path] = generator.sha256;
  const entries = fixtureOnly ? [] : evaluations.entries;
  const packaged = [];
  for (const entry of entries) {
    validateEntry(entry);
    for (const s of [entry.load.scene, entry.load.config, entry.load.task, ...entry.evidence]) {
      assert.equal(hash(await readFile(join(root, s.path))), s.sha256, `Stale leaderboard evidence or recipe: ${s.path}`);
      manifest.inputs[s.path] = s.sha256;
    }
    const [scene, config, task] = await Promise.all([entry.load.scene, entry.load.config, entry.load.task].map(async s => JSON.parse(await readFile(join(root, s.path)))));
    const data = JSON.stringify({scene, config, task, seed: entry.load.seed});
    assert.equal(hash(data), entry.load.asset_sha256, `Tested recipe identity mismatch: ${entry.id}`);
    const id = `tested-${entry.id}`, asset = `data/${id}.json`;
    assert(!catalog.presets.some(p => p.id === id), `Duplicate controller identity: ${id}`);
    await writeFile(join(output, asset), data);
    const preset = {id, label: `Tested recipe · ${entry.name}`, mode: 'embedded', path: asset,
      scene: entry.load.scene.path, config: entry.load.config.path, task: entry.load.task.path,
      asset_sha256: entry.load.asset_sha256,
      description: entry.description, readiness: 'Experimental evaluation · inspect the controller leaderboard for failed and missing gates.',
      evidence: entry.limitations};
    catalog.presets.push(preset); manifest.presets.push({id, mode: 'embedded', path: asset});
    packaged.push({...entry, preset_id: id});
  }
  await writeFile(join(output, 'leaderboard.json'), JSON.stringify({version: 1, entries: packaged, scope: evaluations.scope}));
  manifest.inputs[path] = hash(bytes);
}
