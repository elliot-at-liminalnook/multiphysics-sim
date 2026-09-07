// Capture an explicit policy experiment; no simulation or physics in this tool.
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root=resolve(import.meta.dirname,'../..');
const recipePath='examples/full-robot/joint-feedback-experiment.json';
const out=resolve(process.argv[2]||join(root,'runs/full-robot/learning/joint-feedback'));
const inputHashes={};
async function source(path) {
 const bytes=await readFile(resolve(root,path));inputHashes[path]=createHash('sha256').update(bytes).digest('hex');return bytes.toString();
}
await source('examples/full-robot/prepare_joint_feedback.mjs');
const recipe=JSON.parse(await source(recipePath));
const scene=JSON.parse(await source(recipe.scene));const config=JSON.parse(await source(recipe.config));
const planner=JSON.parse(await source(recipe.bounds_from));const script=await source(recipe.controller_source);
assert.equal(scene.robot.source.cad_sha256,planner.expected_cad_sha256);
assert.deepEqual(config.motors.target_coordinates,planner.independent_coordinates);
assert.equal(planner.bounds.length,planner.independent_coordinates.length);
assert.equal(config.policy,undefined);
config.policy={observation_source:recipe.observation_source,target_bounds_rad:Object.fromEntries(planner.independent_coordinates.map((n,i)=>[n,[planner.bounds[i].lower,planner.bounds[i].upper]]))};
await mkdir(out,{recursive:true});const outputs={};
for(const gain of recipe.gains){
 const variant=structuredClone(scene);variant.period_s=recipe.policy_period_s;
 variant.controller={sources:{entry:'joint-reference-feedback.rhai',files:{'joint-reference-feedback.rhai':script}},parameters:{},inputs:[{...recipe.input,initial:gain}]};
 // Only controller and its sampling period may differ from the source scene.
 assert.deepEqual(variant.robot,scene.robot);assert.deepEqual(variant.options,scene.options);
 for(const [suffix,data] of [['scene',variant],['config',config]]) {
  const name=`gain-${gain}.${suffix}.json`;const bytes=JSON.stringify(data);
  await writeFile(join(out,name),bytes);outputs[name]=createHash('sha256').update(bytes).digest('hex');
 }
 if(recipe.refine_gains.includes(gain)) {
  const refined=structuredClone(config);refined.step_s/=2;refined.steps*=2;refined.report_every*=2;
  const name=`gain-${gain}.refined.config.json`;const bytes=JSON.stringify(refined);
  await writeFile(join(out,name),bytes);outputs[name]=createHash('sha256').update(bytes).digest('hex');
 }
}
await writeFile(join(out,'manifest.json'),JSON.stringify({recipe:recipePath,inputs:inputHashes,outputs,scope:recipe.scope},null,2));
console.log(`Prepared ${recipe.gains.length} captured controller variants in ${out}`);
