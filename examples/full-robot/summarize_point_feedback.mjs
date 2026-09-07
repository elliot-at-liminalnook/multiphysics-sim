// Preserve measured comparisons; never promote the controller merely for completing.
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/point-feedback',browser='runs/interactive/point-feedback',artifacts={};
async function bytes(path){const b=await readFile(path);artifacts[path]=createHash('sha256').update(b).digest('hex');return b;}
async function read(path){return JSON.parse(await bytes(path));}
const manifest=await read(directory+'/manifest.json'),native=[];
for(const prefix of ['','refined.']){
 const c=await read(`${directory}/${prefix}execution.json`),l=await read(`${directory}/${prefix}lift.json`),t=await read(`${directory}/${prefix}tracking.json`);
 assert(c.completed&&c.error===null);assert.equal(t.alignment,'recorded_reference_time');
 const previous=await read('runs/full-robot/learning/body-feedback/'+prefix+'tracking.json');
 const previousByTime=new Map(previous.sample_errors.map(s=>[s.time_s,s]));
 let preActivationDifference=0;
 for(const sample of t.sample_errors.filter(s=>s.reference_time_s<=0.75)){
   const before=previousByTime.get(sample.time_s);assert(before,'same reporting times required');
   const a=sample.errors.world['-Y-foot-surface'].actual_m,b=before.errors.world['-Y-foot-surface'].actual_m;
   preActivationDifference=Math.max(preActivationDifference,Math.hypot(...a.map((v,i)=>v-b[i])));
 }
 assert(preActivationDifference<1e-10,'inactive point feedback must preserve previous foot motion');
 const peak=t.sample_errors.find(s=>Math.abs(s.reference_time_s-1.2)<1e-9);assert(peak);
 native.push({inactive_prefix_maximum_foot_difference_m:preActivationDifference,previous_maximum_world_foot_path_error_m:Math.max(...previous.sample_errors.map(s=>s.errors.world['-Y-foot-surface'].distance_m)),step_s:c.step_s,simulated_s:c.simulated_s,wall_s:c.stepping_wall_s,lift:l.report,
  peak_body_error:peak.reference_link_pose_error,terminal_world_foot:t.sample_errors.at(-1).errors.world['-Y-foot-surface'],
  maximum_world_foot_path_error_m:Math.max(...t.sample_errors.map(s=>s.errors.world['-Y-foot-surface'].distance_m)),
  internal_contact_samples:c.contact_steps.flatMap(s=>s.contacts.filter(x=>x.other!==null)).length,
  maximum_sampled_point_suggestion_rad:Math.max(...c.frames.flatMap(f=>f.policy?.point_feedback?.correction_rad?.map(Math.abs)||[0]))});
}
const comparison=await read(directory+'/refinement.json');
const profile=await read(directory+'/profile.json');
await read(directory+'/profile.config.json');
await read(directory+'/profile.execution.json');
const portability=await read(browser+'/robot-browser.json'),ui=await read(browser+'/viewer-report.json');
assert(portability.passed&&ui.passed);
for(const p of ['scene.json','config.json','refined.config.json'])await read(directory+'/'+p);
await read('examples/full-robot/point-feedback-experiment.json');
for(const p of ['examples/full-robot/prepare_point_feedback.mjs','examples/full-robot/summarize_point_feedback.mjs','examples/full-robot/point-feedback-validation.md',
 'examples/interactive/summarize_embedded_profile.mjs','examples/interactive/controllers/point-position-feedback.rhai','crates/sim-runtime/src/point_feedback.rs','crates/sim-runtime/src/body_feedback.rs','crates/sim-runtime/src/embedded_policy.rs',
 'crates/sim-runtime/src/embedded.rs','crates/sim-runtime/tests/point_feedback.rs',browser+'/native-runner',browser+'/source-manifest.json',
 browser+'/viewer/build-manifest.json',browser+'/viewer/sim_web_bg.wasm','runs/interactive/robot-lab-point-feedback-2026-09-07.zip'])await bytes(p);
await writeFile('examples/full-robot/point-feedback-status.json',JSON.stringify({milestone:'bounded_swing_point_feedback',task_accepted:false,
 scope:'Teacher-state experiment; native/refined task and browser evidence recorded. Not walking, hardware calibration or realtime acceptance.',
 recipe:manifest.recipe,native,refinement:{markers:comparison.markers,contact_pair_mismatch_samples:comparison.contact_pair_mismatch_samples,maximum_sampled_differences:comparison.maximum_sampled_differences},
 portability,ui,profile,artifacts},null,2));
console.log('Saved point-feedback measurements, browser checks and artifact hashes.');
