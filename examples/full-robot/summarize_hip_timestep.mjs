import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const artifacts={};
async function bytes(p){const b=await readFile(p);artifacts[p]=createHash('sha256').update(b).digest('hex');return b;}
async function read(p){return JSON.parse(await bytes(p));}
const roots=['hip-timestep','hip-backlash-screen'].map(n=>'runs/full-robot/learning/'+n);
const manifests=[],analyses=[];
for(const root of roots){
 const m=await read(root+'/manifest.json'),a=await read(root+'/analysis.json');
 for(const [p,h]of Object.entries({...m.inputs,...m.outputs,...a.inputs})){await bytes(p);assert.equal(artifacts[p],h,p);}
 for(const c of m.cases){const d=await read(root+'/'+c.name+'.window.json');assert(d.window_complete&&d.error===null&&d.frames.length===951);}
 await bytes(root+'/hip-series.json');manifests.push(m);analyses.push(a);
}
const original=await read(manifests[0].scene),screen=await read(manifests[1].scene);
const joint=original.robot.joints.find(j=>j.name==='-Y | Hip servo output');
const before=joint.physics.backlash;assert.equal(before,manifests[1].overrides[0].before);
const provenance={joint:joint.name,physics_source:joint.physics.source,clearance_m:joint.physics.clearance,lever_m:joint.physics.lever,inferred_backlash_rad:before,
 source_formula:'clearance / max(COM lever length, 0.005 m)',runtime_interpretation:'full output-side rotational gap added to motor gearbox backlash'};
joint.physics.backlash=0;assert(isDeepStrictEqual(original,screen),'sensitivity scene changed more than one backlash field');
for(let i=0;i<manifests[0].cases.length;i++)assert(isDeepStrictEqual(await read(manifests[0].cases[i].config),await read(manifests[1].cases[i].config)),'screen changed controller or integrator configuration');
const viewerRoot='runs/interactive/hip-timestep';
const ui=await read(viewerRoot+'/viewer-report.json'),browser=await read(viewerRoot+'/fixture-browser.json');
const tests=await read(viewerRoot+'/test-results.json');
assert(ui.passed&&browser.passed);
assert.equal(tests.session_tests.failed,0);assert(tests.cli.passed);
for(const p of ['cad/robocad/physical.py','cad/PHYSICAL_MODEL.md','crates/sim-domain-robot/src/motor.rs','crates/sim-runtime/src/embedded.rs',
 'crates/sim-runtime/tests/embedded_session.rs','crates/sim-runtime/examples/capture_embedded_window.rs','.github/workflows/browser.yml',
 'examples/full-robot/prepare_hip_timestep_trace.mjs','examples/full-robot/analyze_hip_timestep_trace.mjs','examples/full-robot/prepare_hip_backlash_screen.mjs',
 'examples/interactive/check_capture_window.mjs','examples/full-robot/hip-timestep-validation.md',import.meta.filename,
 'target/release/examples/capture_embedded_window',viewerRoot+'/viewer/build-manifest.json',viewerRoot+'/viewer/sim_web_bg.wasm'])await bytes(p);
const report={scope:'Observation and single-parameter sensitivity experiment, not a physical-model or controller promotion.',training_model_accepted:false,realtime_accepted:false,
 physical_provenance:provenance,override:manifests[1].overrides[0],original:analyses[0],screen:analyses[1],
 validation:{tests,browser_fixture:browser,viewer:ui},artifacts};
await writeFile('examples/full-robot/hip-timestep-status.json',JSON.stringify(report,null,2));
console.log(JSON.stringify({passed:true,only_one_physical_field_changed:true,artifacts:Object.keys(artifacts).length,viewer_checks:ui.checks.length}));
