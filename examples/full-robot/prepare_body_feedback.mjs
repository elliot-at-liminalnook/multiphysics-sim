// Configure shared Rust feedback and a Rhai policy; no simulated motion here.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const hashes={};
async function read(path,json=true){const b=await readFile(path);hashes[path]=createHash('sha256').update(b).digest('hex');return json?JSON.parse(b):b.toString();}
const recipe=await read('examples/full-robot/body-feedback-experiment.json');
const scene=await read(recipe.scene),config=await read(recipe.config),plan=await read(recipe.plan),script=await read(recipe.controller,false);
assert(plan.completed);assert.deepEqual(scene.robot.source,plan.source);assert.deepEqual(config.motors.target_trajectory,plan.trajectory);
const reference_link=config.policy.task_observations.reference_link;
const initial=plan.frames[0].poses.find(p=>p.name===reference_link).position_m;
const path=structuredClone(plan.config.base_displacements_world_m);
for(const k of path.keyframes)k.values=k.values.map((v,i)=>v+initial[i]);
config.policy.body_feedback={expected_cad_sha256:scene.robot.source.cad_sha256,coordinate_frame:plan.markers.coordinate_frame,reference_link,
 support_markers:config.policy.task_observations.markers,position_world_m:path,
 velocity_damping_s:recipe.velocity_damping_s,damping_m_per_rad:recipe.damping_m_per_rad,
 maximum_correction_rad:recipe.maximum_correction_rad,full_support_force_n:recipe.full_support_force_n};
scene.controller.sources={entry:'body-position-feedback.rhai',files:{'body-position-feedback.rhai':script}};
scene.controller.inputs.push({name:'command.body_gain',kind:'Dimensionless',lower:0,upper:1,initial:recipe.body_gain});
const refined=structuredClone(config);refined.step_s/=2;refined.steps*=2;refined.report_every*=2;
await read('examples/full-robot/prepare_body_feedback.mjs',false);
await mkdir(recipe.output_directory,{recursive:true});const outputs={};
for(const [name,value] of [['scene.json',scene],['config.json',config],['refined.config.json',refined]]){const b=JSON.stringify(value);outputs[name]=createHash('sha256').update(b).digest('hex');await writeFile(recipe.output_directory+'/'+name,b);}
await writeFile(recipe.output_directory+'/manifest.json',JSON.stringify({recipe,inputs:hashes,outputs},null,2));
console.log('Prepared bounded body feedback with gain '+recipe.body_gain);
