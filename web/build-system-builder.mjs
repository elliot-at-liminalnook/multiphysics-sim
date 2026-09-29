// Package the browser system runner: page, wasm-bindgen glue for the shared
// runtime, the example system files and every authored part source.
import { readFile, writeFile, mkdir, cp, readdir } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { execFileSync } from 'node:child_process';
const root = resolve(import.meta.dirname, '..');
const output = resolve(root, process.argv[2] || 'runs/interactive/system-builder');
const wasm = process.env.WASM_ARTIFACT || 'target/wasm32-unknown-unknown/release/sim_web.wasm';
await mkdir(join(output, 'data'), { recursive: true });
for (const f of ['index.html', 'system.js']) await cp(join(root, 'web/system-builder', f), join(output, f));
await cp(join(root, 'web/serve-viewer.mjs'), join(output, 'serve-viewer.mjs'));
execFileSync(process.env.WASM_BINDGEN || 'wasm-bindgen', [resolve(root, wasm), '--target', 'web', '--out-name', 'sim_web', '--out-dir', output], { stdio: 'inherit' });
const systems = [
  { id: 'worm-winch', label: 'Worm-drive winch', path: 'examples/systems-builder/worm-drive/winch.system.json' },
  { id: 'gearmotor-winch', label: 'Gearmotor winch (nested parts)', path: 'examples/systems-builder/parts-from-parts/winch.system.json' },
  { id: 'leg', label: 'One-joint leg (authored gravity part)', path: 'examples/systems-builder/parts-from-parts/leg.system.json' },
];
for (const s of systems) s.text = await readFile(join(root, s.path), 'utf8');
const parts = {};
for (const f of (await readdir(join(root, 'library/parts'))).filter(f => f.endsWith('.part'))) parts[f] = await readFile(join(root, 'library/parts', f), 'utf8');
await writeFile(join(output, 'data/catalog.json'), JSON.stringify({ systems, parts }));
console.log(`System runner: node ${join(output, 'serve-viewer.mjs')} ${output} 4174`);
