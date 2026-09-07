// Configuration derivation only; shared Rust computes kinematics and dynamics.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const hashes={};
async function read(path,json=true){const b=await readFile(path);hashes[path]=createHash('sha256').update(b).digest('hex');return json?JSON.parse(b):b.toString();}
const recipe=await read('examples/full-robot/point-feedback-experiment.json');
const scene=await read(recipe.scene),config=await read(recipe.config),plan=await read(recipe.plan),script=await read(recipe.controller,false);
assert(plan.completed);assert.deepEqual(scene.robot.source,plan.source);assert.deepEqual(config.motors.target_trajectory,plan.trajectory);
const indices=recipe.marker_ids.map(id=>{const i=plan.markers.markers.findIndex(m=>m.id===id);assert(i>=0);return i;});
assert.equal(new Set(indices).size,indices.length);
const path=structuredClone(plan.config.displacements_world_m);
for(const k of path.keyframes)k.values=indices.flatMap(i=>k.values.slice(3*i,3*i+3).map((v,axis)=>v+plan.frames[0].marker_positions_world_m[i][axis]));
config.policy.point_feedback={expected_cad_sha256:scene.robot.source.cad_sha256,coordinate_frame:plan.markers.coordinate_frame,
 markers:indices.map(i=>plan.markers.markers[i]),position_world_m:path,activation:recipe.activation,
 damping_m_per_rad:recipe.damping_m_per_rad,maximum_correction_rad:recipe.maximum_correction_rad};
scene.controller.sources={entry:'point-position-feedback.rhai',files:{'point-position-feedback.rhai':script}};
scene.controller.inputs.push({name:'command.point_gain',kind:'Dimensionless',lower:0,upper:1,initial:recipe.point_gain});
const refined=structuredClone(config);refined.step_s/=2;refined.steps*=2;refined.report_every*=2;
await read('examples/full-robot/prepare_point_feedback.mjs',false);
await mkdir(recipe.output_directory,{recursive:true});const outputs={};
for(const [name,value] of [['scene.json',scene],['config.json',config],['refined.config.json',refined]]){const b=JSON.stringify(value);outputs[name]=createHash('sha256').update(b).digest('hex');await writeFile(recipe.output_directory+'/'+name,b);}
await writeFile(recipe.output_directory+'/manifest.json',JSON.stringify({recipe,inputs:hashes,outputs},null,2));
console.log('Prepared bounded point feedback with gain '+recipe.point_gain);
