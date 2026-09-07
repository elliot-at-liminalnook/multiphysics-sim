// Summarize Rust/CAD/browser measurements; this does not define new acceptance gates.
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const directory='runs/full-robot/learning/forward-slow',browser='runs/interactive/forward-slow';
const artifacts={};
async function read(path){const b=await readFile(path);artifacts[path]=createHash('sha256').update(b).digest('hex');return JSON.parse(b);}
const recipe=await read('examples/full-robot/forward-rate-experiment.json');
const manifest=await read(directory+'/manifest.json');
const native=[];
for(const [variant,prefix] of [['baseline',''],['refined','refined.'],['coarse','coarse.']]){
 const c=await read(`${directory}/${prefix}execution.json`),t=await read(`${directory}/${prefix}tracking.json`),l=await read(`${directory}/${prefix}lift.json`);
 assert(c.completed&&c.error===null);assert.equal(t.alignment,'recorded_reference_time');
 await read(`${directory}/${prefix}config.json`);
 const contacts=c.contact_steps.flatMap(s=>s.contacts.filter(p=>p.other!==null).map(p=>({...p,time_s:s.start_time_s+s.step_s})));
 const clock=c.motion_gate.clock;
 const waiting=c.motion_gate_trace.filter(s=>!s.clock.advancing&&!s.clock.timed_out&&s.clock.reference_tick*clock.period_s<clock.duration_s-1e-10);
 native.push({variant,simulated_s:c.simulated_s,step_s:c.step_s,wall_s:c.stepping_wall_s,lift:l.report,
  terminal_world_foot:t.sample_errors.at(-1).errors.world['-Y-foot-surface'],support_wait_s:waiting.length*clock.period_s,
  body_error_at_peak:t.sample_errors.find(s=>Math.abs(s.time_s-1.2)<1e-10).reference_link_pose_error,
  internal_contacts:{samples:contacts.length,first_endpoint_s:contacts[0]?.time_s??null,last_endpoint_s:contacts.at(-1)?.time_s??null,
   maximum_penetration_m:Math.max(0,...contacts.map(p=>p.penetration_m)),maximum_force_n:Math.max(0,...contacts.map(p=>Math.hypot(...p.force_n)))}});
}
const comparisons={};
for(const name of ['refinement','coarse-comparison']){
 const r=await read(`${directory}/${name}.json`);
 comparisons[name]={markers:r.markers,contact_pair_mismatch_samples:r.contact_pair_mismatch_samples,
  maximum_sampled_differences:r.maximum_sampled_differences,scope:r.scope};
}
const geometry=await read(directory+'/hip-contact-cells.json');
const contactProbes=geometry.frames.map(f=>{
 const point=f.contact_point_probes.find(p=>p.runtime_contact),nodes=f.contact_point_probes.filter(p=>p.sdf_cell_node);
 const nearest=Math.min(...point.brep_members.map(m=>m.distance_mm));
 return {time_s:f.time_s,reported_penetration_m:point.runtime_contact.penetration_m,nearest_cad_surface_mm:nearest,
  any_cad_solid_inside:point.brep_members.some(m=>m.solid_states.includes('inside')),
  grid_nodes:nodes.map(p=>({index:p.sdf_cell_node.index,stored_distance_m:p.sdf_cell_node.stored_distance_m,
   any_cad_solid_inside:p.brep_members.some(m=>m.solid_states.includes('inside'))}))};
});
const portability=await read(browser+'/robot-browser.json'),ui=await read(browser+'/viewer-report.json');
assert(portability.passed&&ui.passed,'browser delivery checks must pass');
await read(browser+'/viewer/build-manifest.json');
for(const p of ['plan.json','planner.json','lift-requirements.json','inspection-times.json','scene.json'])await read(directory+'/'+p);
for(const p of ['examples/full-robot/prepare_forward_rate.mjs','examples/full-robot/summarize_forward_rate.mjs',
 'cad/robocad/capture_geometry.py','cad/tests/test_capture_geometry.py','examples/full-robot/audit_captured_geometry.py',
 'crates/sim-runtime/src/motion_tracking.rs','crates/sim-runtime/tests/motion_tracking.rs',
 'cad/robocad/physical.py','cad/robocad/kernel/base.py','cad/robocad/kernel/occt.py','cad/tests/test_physical.py',
 'examples/full-robot/forward-rate-validation.md','examples/full-robot/chassis-sign-correction-experiment.json',
 'runs/interactive/forward-slow/source-manifest.json','runs/interactive/robot-lab-slower-placement-2026-09-07.zip',
 'runs/interactive/landing-checkpoint/native-runner','runs/interactive/landing-checkpoint/source-manifest.json']){
 artifacts[p]=createHash('sha256').update(await readFile(p)).digest('hex');
}
await writeFile('examples/full-robot/forward-rate-status.json',JSON.stringify({milestone:'slower_forward_placement_experiment',task_accepted:false,
 scope:'Sampled supported lift improves; false grid contact at inspected points and timestep sensitivity prevent placement/collision promotion. No walking or hardware accuracy acceptance.',
 recipe,maximum_motor_knot_difference_rad:manifest.maximum_motor_knot_difference_rad,native,comparisons,
 contact_probes:contactProbes,geometry_scope:geometry.scope,portability,ui,artifacts},null,2));
console.log('Wrote measured forward-rate status; task_accepted remains false.');
