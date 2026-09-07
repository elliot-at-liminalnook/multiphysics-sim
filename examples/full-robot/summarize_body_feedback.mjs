// Collect existing shared-runtime task measurements and source identities.
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/body-feedback',browser='runs/interactive/body-feedback',artifacts={};
async function bytes(path){const b=await readFile(path);artifacts[path]=createHash('sha256').update(b).digest('hex');return b;}
async function read(path){return JSON.parse(await bytes(path));}
const manifest=await read(directory+'/manifest.json'),native=[];
for(const prefix of ['','refined.']){
 const c=await read(`${directory}/${prefix}execution.json`),l=await read(`${directory}/${prefix}lift.json`),t=await read(`${directory}/${prefix}tracking.json`);
 assert(c.completed&&c.error===null);assert.equal(t.alignment,'recorded_reference_time');
 const peak=t.sample_errors.find(s=>Math.abs(s.reference_time_s-1.2)<1e-9);
 assert(peak,'peak reference phase must be inspected exactly');
 native.push({step_s:c.step_s,simulated_s:c.simulated_s,wall_s:c.stepping_wall_s,lift:l.report,
  peak_body_error:peak.reference_link_pose_error,terminal_world_foot:t.sample_errors.at(-1).errors.world['-Y-foot-surface'],
  internal_contact_samples:c.contact_steps.flatMap(s=>s.contacts.filter(x=>x.other!==null)).length,
  maximum_sampled_suggestion_rad:Math.max(...c.frames.flatMap(f=>f.policy?.body_feedback?.correction_rad?.map(Math.abs)||[0]))});
}
const comparison=await read(directory+'/refinement.json');
const portability=await read(browser+'/robot-browser.json'),ui=await read(browser+'/viewer-report.json');
assert(portability.passed&&ui.passed);
for(const p of ['scene.json','config.json','refined.config.json'])await read(directory+'/'+p);
await read('examples/full-robot/body-feedback-experiment.json');
for(const p of ['examples/full-robot/prepare_body_feedback.mjs','examples/full-robot/summarize_body_feedback.mjs','examples/full-robot/body-feedback-validation.md',
 'examples/interactive/controllers/body-position-feedback.rhai','crates/sim-runtime/src/body_feedback.rs','crates/sim-runtime/src/embedded_policy.rs',
 'crates/sim-runtime/src/embedded.rs','crates/sim-runtime/tests/body_feedback.rs',browser+'/native-runner',browser+'/source-manifest.json',
 browser+'/viewer/build-manifest.json',browser+'/viewer/sim_web_bg.wasm','runs/interactive/robot-lab-body-feedback-2026-09-07.zip'])await bytes(p);
await writeFile('examples/full-robot/body-feedback-status.json',JSON.stringify({milestone:'bounded_body_translation_feedback',task_accepted:false,
 scope:'Both timesteps improve sampled body and final foot errors and pass supported lift; full foot-path sensitivity remains. Not walking, hardware calibration or realtime acceptance.',
 recipe:manifest.recipe,native,refinement:{markers:comparison.markers,contact_pair_mismatch_samples:comparison.contact_pair_mismatch_samples,maximum_sampled_differences:comparison.maximum_sampled_differences},
 portability,ui,artifacts},null,2));
console.log('Saved body-feedback measurements, browser checks and artifact hashes.');
