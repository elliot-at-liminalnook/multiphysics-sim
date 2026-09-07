import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/auxiliary-coloring';
const source='runs/full-robot/learning/auxiliary-condensation';
const inputs={},outputs={};const hash=b=>createHash('sha256').update(b).digest('hex');
async function read(path){const b=await readFile(path);inputs[path]=hash(b);return JSON.parse(b);}
await mkdir(directory,{recursive:true});
const previous=await read(source+'/manifest.json');
const scene=await read(previous.scene);
assert.equal(scene.options.motor_dynamics??'detailed','detailed');
for(const name of ['base','refined']){
  const c=await read(`${source}/${name}.config.json`);
  assert.equal(c.implicit.condense_auxiliary,true);
  assert.equal(c.implicit.color_auxiliary_jacobian,undefined);
  c.implicit.color_auxiliary_jacobian=true;
  const b=JSON.stringify(c);await writeFile(`${directory}/${name}.config.json`,b);outputs[name]=hash(b);
  if(name==='base'){
    const viewer=structuredClone(c);viewer.profile_solver=false;
    const data=JSON.stringify(viewer);await writeFile(`${directory}/viewer.config.json`,data);outputs.viewer=hash(data);
  }
}
inputs[import.meta.filename]=hash(await readFile(import.meta.filename));
await writeFile(directory+'/manifest.json',JSON.stringify({
  scope:'Only the inner numerical derivative assembly changes. Same detailed laws, mechanical/auxiliary coordinates, tolerances, events, timestep, CAD, world and controller as the uncolored condensation experiment. Independence is declared by the registered servo/driver adapter; unknown adapters use ordinary probes.',
  scene:previous.scene,inputs,outputs,
  comparisons:{base:source+'/base.execution.json',refined:source+'/refined.execution.json'},
  expected_structural_dimension:48,expected_colors:4,
  promotion:'Require complete trajectories, numerical equivalence including event schedules and forces, and lower total runtime. This experiment cannot by itself repair the parent condensation/reference discrepancy or establish walking/realtime.',
},null,2));
console.log('Prepared auxiliary coloring comparisons');
