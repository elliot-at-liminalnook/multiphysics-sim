// Package existing Rust outputs and immutable captures; no browser-side physics.
import { readFile, writeFile, mkdir, cp } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve, join, dirname, relative, isAbsolute } from 'node:path';
import { execFileSync } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';
import { packageLeaderboard } from './leaderboard/package.mjs';
const root = resolve(import.meta.dirname, '..');
const output = resolve(root, process.argv[2] || 'runs/interactive/viewer');
const fixtureOnly = process.argv.includes('--fixture-only');
const environmentOnly = process.argv.includes('--environment-only');
const onlyPreset = process.argv.find(a=>a.startsWith('--preset='))?.slice(9);
const wasmArtifact = process.env.WASM_ARTIFACT || 'target/wasm32-unknown-unknown/release/sim_web.wasm';
const read = async p => JSON.parse(await readFile(resolve(root,p)));
const hash = async p => createHash('sha256').update(await readFile(resolve(root,p))).digest('hex');
await mkdir(output, { recursive:true }); await mkdir(join(output,'data'), { recursive:true });
for (const file of ['index.html','viewer.css','viewer.js','leaderboard.js','leaderboard-model.mjs','leaderboard.css','video-export.js','motion-commands.mjs','hardware-sync.mjs','drive-input.mjs','drive-panel.mjs']) await cp(join(root,'web/viewer',file),join(output,file));
await cp(join(root,'web/worker.js'),join(output,'worker.js'));
await cp(join(root,'web/worker-message.mjs'),join(output,'worker-message.mjs'));
await cp(join(root,'web/serve-viewer.mjs'),join(output,'serve-viewer.mjs'));
await writeFile(join(output,'OPEN.txt'), 'Robot motion workspace\n\nRequires Node.js 22 or newer. In this directory run:\n  node serve-viewer.mjs . 4173\nThen open http://127.0.0.1:4173\n\nAlternatively serve the directory through a static HTTPS host. File URLs do not support this worker setup. All browser assets are local.\n\nPresets identify live Rust/WASM physics versus recorded robot experiments. Detailed and effective-servo profiles execute live Rust physics with different accuracy and speed tradeoffs. The effective-servo preset is a single-foot experiment; the two-cycle crawl preset executes eight supported swings. The reversal-aware WASD crawl completes tested direction changes and a 60-second rendered run with 28 supported swings. Active crawling keeps up with realtime on the documented Mac, but p95 latency is 29 ms against a 20 ms target. It travels about 70 mm in that minute; general command sequences, useful walking speed and uneven terrain remain unvalidated. The terrain-contact experiment retains floor/height-field contact but omits link-to-link impact forces, with independent sampled overlap checks. It reproduces the tested native crawl and reports 21.6 ms active p95, still above the 20 ms target. General commands and uneven-terrain walking remain unvalidated. The teacher motor-correction preset adds twelve bounded angle offsets, ideal teacher observations, and a provisional reward to the baseline crawl. Zero corrections reproduce the baseline minute; small probes pass stepping checks. Expand Motor corrections to inspect or clear them. That baseline preset has no trained neural policy, and arbitrary corrections remain unvalidated. The first neural-teacher preset runs a small trained Rust network on top of that baseline. Its initial search shows only a tiny reward improvement; robustness and hardware transfer remain unverified. Learned corrections can be inspected live. The one-minute recipe allows 80 solver iterations at unchanged tolerances; sustained validation and timing are reported separately. The distilled-student preset replaces body/foot motor feedback with a trained network using proposed ideal encoder/IMU-style observations. Unused teacher feedback calculations are omitted with an exact trajectory check. Its minute completes 28 supported swings, but final stopping error remains above the 1 mm gate. Actual sensors and the planner state estimator are still unconfirmed; this is not a deployable hardware controller. The earlier online prototype and fixed crawl remain available. Some bundles also include recorded comparisons. Sustained terrain walking, robust learned policies and hardware accuracy remain unfinished.\n\nThe improved-student preset searches neural weights across three development episodes. Those checks pass, including 28 swings and 0.685 mm final stopping error over a minute. A reserved opposite-direction push and an additional 5 ms physics refinement each miss one swing. It remains experimental, with ideal observations and a privileged planner.\n\nThe paced-student preset keeps the improved network and changes weight shifts and foot timing, with optional bounded implicit-step recovery. Its unforced minute passes 26 supported swings; a three-push minute misses the final heading gate. This is experimental flat-floor walking at only 1.25 mm/s requested speed, with ideal observations and a privileged planner. See the preset status and versioned step-margin reports for measured browser performance and remaining limitations.\n\nThe efficient-student preset preserves the paced controller and physical force laws while reusing solver derivatives. A bounded first attempt can restart from the original state with fresh derivatives before timestep subdivision. The native minute agrees closely with the paced baseline and needs 40% fewer Jacobian builds. The rendered minute keeps up with realtime, but active p95 is 22.5 ms against the 20 ms target. Heading and timestep sensitivity remain unresolved.\n\nThe heading-student preset adds a heading-aware walking task and selects improved student weights across short and minute-long walks, including a 5 ms physics reference. It retains the efficient mechanical solver and the same provisional physical model. The viewer shows heading error alongside body tracking and qualified steps. This does not supply a heading estimator to the policy. Independent validation and rendered timing are reported in examples/full-robot/heading-task in the source repository; general recovery, useful walking speed, terrain and hardware transfer remain unfinished.\n\nThe browser-solver preset preserves the heading controller and physical model, while using independent constraint-block factorization, 1e-5 numerical precision and bounded solver recovery. The viewer avoids redrawing unchanged scenes. Its rendered minute keeps up with realtime but active p95 20.98 ms still misses 20 ms; a tested forward/turn/reverse/stop case passes both timing targets at 18.97 ms active p95. This remains a slow flat-floor experiment with ideal observations and a privileged planner. See examples/full-robot/browser-precision for the source configurations and full validation reports.\n\nThe build manifest hashes the source inputs. Keep it with the bundle.\n');
await mkdir(join(output,'vendor/addons/controls'), { recursive:true });
await cp(join(root,'web/node_modules/three/LICENSE'),join(output,'vendor/three-LICENSE'));
for (const file of ['three.module.js','three.core.js']) await cp(join(root,'web/node_modules/three/build',file),join(output,'vendor',file));
await cp(join(root,'web/node_modules/three/examples/jsm/controls/OrbitControls.js'),join(output,'vendor/addons/controls/OrbitControls.js'));
const bindgen = process.env.WASM_BINDGEN || 'wasm-bindgen';
execFileSync(bindgen,[resolve(root,wasmArtifact),'--target','web','--out-name','sim_web','--out-dir',output],{stdio:'inherit'});
const configured = await read('web/viewer/presets.json'); const catalog = {presets:[]}; const manifest = {inputs:{},presets:[]};
if(onlyPreset && !configured.presets.some(p=>p.id===onlyPreset))throw Error(`Unknown preset: ${onlyPreset}`);
manifest.wasm = {path: wasmArtifact, sha256: await hash(wasmArtifact), browser_module_sha256: await hash(join(output, 'sim_web_bg.wasm')),
  bindgen_version: execFileSync(bindgen, ['--version'], {encoding: 'utf8'}).trim()};
if (process.env.WASM_BUILD_MANIFEST) {
  const build = await read(process.env.WASM_BUILD_MANIFEST);
  if (!build.completed || build.artifact?.sha256 !== manifest.wasm.sha256) throw Error('WASM build manifest does not match the packaged artifact');
  manifest.wasm.build = build; manifest.inputs[process.env.WASM_BUILD_MANIFEST] = await hash(process.env.WASM_BUILD_MANIFEST);
}
// A drive-profile preset ships the model, the binding and every file the
// binding names for its embedded program, at their repository-relative paths
// under data/<id>/, so the page resolves each listed path against the
// binding's URL exactly as Rust names it. The packager reads only the
// binding's path fields (drive_profile, embedded.entry/files/config); the
// page asks Rust (drive_binding_files) what to fetch, so a file missed here
// fails loudly at load. Missing files or paths outside the repository fail packaging.
async function packageDrive(preset) {
  const {model, binding} = preset.drive ?? {};
  if (typeof model !== 'string' || typeof binding !== 'string') throw new Error(`preset ${preset.id}: drive.model and drive.binding must be repository paths`);
  const inside = path => { const rel = relative(root, resolve(root, path)); if (!rel || rel.startsWith('..') || isAbsolute(rel)) throw new Error(`preset ${preset.id}: ${path} is outside the repository`); return rel.split('\\').join('/'); };
  const parsed = await read(binding);
  const embedded = parsed.embedded;
  if (typeof parsed.drive_profile !== 'string') throw new Error(`${binding}: drive_profile is missing`);
  if (!embedded || typeof embedded.entry !== 'string' || typeof embedded.config !== 'string' || (embedded.files !== undefined && !Array.isArray(embedded.files)))
    throw new Error(`${binding}: embedded.entry and embedded.config are required for the browser drive (it cannot start the external controller)`);
  const named = [parsed.drive_profile, embedded.entry, ...(embedded.files ?? []), embedded.config];
  const paths = [inside(model), inside(binding), ...named.map(rel => inside(join(dirname(binding), rel)))];
  const packaged = [];
  for (const path of [...new Set(paths)]) {
    const sha256 = await hash(path); manifest.inputs[path] = sha256;
    await mkdir(dirname(join(output, 'data', preset.id, path)), { recursive: true });
    await cp(resolve(root, path), join(output, 'data', preset.id, path));
    packaged.push({path, sha256});
  }
  return { version: 1, kind: 'drive_files',
    model: { path: inside(model), url: `data/${preset.id}/${inside(model)}` },
    binding: { path: inside(binding), url: `data/${preset.id}/${inside(binding)}` },
    seed: preset.drive.seed ?? 0, packaged,
    scope: 'Source files only. The page fetches them; Rust (sim-web) lists, parses and builds the drive, runs its embedded adapter, limits and deadman. Browser compatibility path, unexecuted.' };
}
for (const preset of configured.presets) {
  if (onlyPreset && preset.id!==onlyPreset) continue;
  if (environmentOnly && !preset.task && preset.mode !== 'live') continue;
  if (fixtureOnly && preset.mode !== 'live' && !preset.fixture) continue;
  if (preset.mode === 'drive') {
    const path=`data/${preset.id}.json`; await writeFile(join(output,path),JSON.stringify(await packageDrive(preset)));
    catalog.presets.push({...preset,path}); manifest.presets.push({id:preset.id,mode:preset.mode,path});
    continue;
  }
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
if(!onlyPreset)await packageLeaderboard(root, output, catalog, manifest, fixtureOnly);
if(onlyPreset){
  const preset=catalog.presets[0];
  await writeFile(join(output,'OPEN.txt'),`${preset.label}\n\nRequires Node.js 22 or newer. In this directory run:\n  node serve-viewer.mjs . 4173\nThen open http://127.0.0.1:4173/?preset=${preset.id}\n\n${preset.description||''}\n\n${preset.readiness||''}\n\n${preset.evidence||''}\n\nKeep build-manifest.json with this bundle. File URLs do not support the Rust worker.\n`);
}
await writeFile(join(output,'catalog.json'),JSON.stringify(catalog,null,2));
for (const path of ['web/leaderboard/package.mjs','web/viewer/leaderboard.js','web/viewer/leaderboard-model.mjs','web/viewer/leaderboard.css','web/viewer/video-export.js']) manifest.inputs[path]=await hash(path);
for (const path of ['web/build-viewer.mjs','web/serve-viewer.mjs','web/viewer/presets.json','web/viewer/viewer.js','web/viewer/motion-commands.mjs','web/viewer/hardware-sync.mjs','web/viewer/drive-input.mjs','web/viewer/drive-panel.mjs','web/viewer/viewer.css','web/viewer/index.html','web/worker.js','web/worker-message.mjs','web/package-lock.json',wasmArtifact]) manifest.inputs[path]=await hash(path);
await writeFile(join(output,'build-manifest.json'),JSON.stringify(manifest,null,2));
console.log(`Packaged ${catalog.presets.length} presets in ${output}`);
