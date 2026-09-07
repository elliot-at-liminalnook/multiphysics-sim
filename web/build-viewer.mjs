// Package existing Rust outputs and immutable captures; no browser-side physics.
import { readFile, writeFile, mkdir, cp } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';
const root = resolve(import.meta.dirname, '..');
const output = resolve(root, process.argv[2] || 'runs/interactive/viewer');
const fixtureOnly = process.argv.includes('--fixture-only');
const environmentOnly = process.argv.includes('--environment-only');
const read = async p => JSON.parse(await readFile(resolve(root,p)));
const hash = async p => createHash('sha256').update(await readFile(resolve(root,p))).digest('hex');
await mkdir(output, { recursive:true }); await mkdir(join(output,'data'), { recursive:true });
for (const file of ['index.html','viewer.css','viewer.js']) await cp(join(root,'web/viewer',file),join(output,file));
await cp(join(root,'web/worker.js'),join(output,'worker.js'));
await cp(join(root,'web/serve-viewer.mjs'),join(output,'serve-viewer.mjs'));
await writeFile(join(output,'OPEN.txt'), 'Robot motion workspace\n\nRequires Node.js 22 or newer. In this directory run:\n  node serve-viewer.mjs . 4173\nThen open http://127.0.0.1:4173\n\nAlternatively serve the directory through a static HTTPS host. File URLs do not support this worker setup. All browser assets are local.\n\nPresets identify live Rust/WASM physics versus recorded robot experiments. The live quadruped lift executes the same Rust controller recipe as the native experiment and runs slower than realtime. Recorded comparison presets are also included. Walking commands, learned policies and hardware accuracy remain unfinished.\n\nThe build manifest hashes the source inputs. Keep it with the bundle.\n');
await mkdir(join(output,'vendor/addons/controls'), { recursive:true });
await cp(join(root,'web/node_modules/three/LICENSE'),join(output,'vendor/three-LICENSE'));
for (const file of ['three.module.js','three.core.js']) await cp(join(root,'web/node_modules/three/build',file),join(output,'vendor',file));
await cp(join(root,'web/node_modules/three/examples/jsm/controls/OrbitControls.js'),join(output,'vendor/addons/controls/OrbitControls.js'));
execFileSync(process.env.WASM_BINDGEN || 'wasm-bindgen',[join(root,'target/wasm32-unknown-unknown/release/sim_web.wasm'),'--target','web','--out-dir',output],{stdio:'inherit'});
const configured = await read('web/viewer/presets.json'); const catalog = {presets:[]}; const manifest = {inputs:{},presets:[]};
for (const preset of configured.presets) {
  if (environmentOnly && !preset.task && preset.mode !== 'live') continue;
  if (fixtureOnly && preset.mode !== 'live' && !preset.fixture) continue;
  const scene = await read(preset.scene); const sceneHash = await hash(preset.scene); manifest.inputs[preset.scene] = sceneHash;
  let data = scene;
  if (preset.mode === 'embedded') {
    data = {scene,config:await read(preset.config)};
    manifest.inputs[preset.config]=await hash(preset.config);
    if (preset.task) { data.task=await read(preset.task); manifest.inputs[preset.task]=await hash(preset.task); }
  }
  if (preset.mode === 'recorded') {
    const capture = await read(preset.capture);
    if (capture.completed !== true || capture.error != null ||
        !isDeepStrictEqual(capture.source,scene.robot.source) ||
        !Object.entries(scene.options).every(([key,value])=>isDeepStrictEqual(capture.scene_options?.[key],value)) ||
        (capture.world && !isDeepStrictEqual(capture.world,scene.robot.world)))
      throw new Error(`Incomplete/mismatched capture: ${preset.capture}`);
    const captureHash = await hash(preset.capture); manifest.inputs[preset.capture] = captureHash;
    data = { version:1, kind:'recorded_physics_view', source:capture.source,
      scene_sha256:sceneHash, capture_sha256:captureHash, simulated_s:capture.simulated_s, stepping_wall_s:capture.stepping_wall_s,
      coordinate_names:capture.independent_coordinates, joint_indices:capture.independent_joint_indices,
      motor_experiment:capture.motor_experiment, initial_coordinates:capture.initial_coordinates,
      effective_scene_options:capture.scene_options,
      initial_base_translation_m:capture.initial_base_translation_m,
      robot:{source:scene.robot.source,world:scene.robot.world,links:scene.robot.links.map(l=>({name:l.name,com:l.com,collision:{vertices:l.collision.vertices,triangles:l.collision.triangles}}))},
      frames:capture.frames.map(f=>({time_s:f.time_s,poses:f.poses,joint_positions:f.joint_positions,servo_targets_rad:f.servo_targets_rad,reference_targets_rad:f.reference_targets_rad,contacts:f.contacts})),
      world_recorded_in_capture:Boolean(capture.world),
      scope:'Read-only recorded rigid poses and telemetry; no live quadruped physics or controller execution in this view. Historical captures rely on the declared original scene for world provenance.'};
  }
  const path=`data/${preset.id}.json`; await writeFile(join(output,path),JSON.stringify(data));
  catalog.presets.push({...preset,path}); manifest.presets.push({id:preset.id,mode:preset.mode,path});
}
await writeFile(join(output,'catalog.json'),JSON.stringify(catalog,null,2));
for (const path of ['web/build-viewer.mjs','web/serve-viewer.mjs','web/viewer/presets.json','web/viewer/viewer.js','web/viewer/viewer.css','web/viewer/index.html','web/worker.js','web/package-lock.json','target/wasm32-unknown-unknown/release/sim_web.wasm']) manifest.inputs[path]=await hash(path);
await writeFile(join(output,'build-manifest.json'),JSON.stringify(manifest,null,2));
console.log(`Packaged ${catalog.presets.length} presets in ${output}`);
