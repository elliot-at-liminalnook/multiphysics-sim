import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/auxiliary-condensation';
const source='runs/full-robot/learning/sample-reuse';
const inputs={},outputs={};
const hash=b=>createHash('sha256').update(b).digest('hex');
async function read(path) {
  const b=await readFile(path); inputs[path]=hash(b); return JSON.parse(b);
}
await mkdir(directory,{recursive:true});
for(const [name,input] of [
  ['base',source+'/final-refresh.base.config.json'],
  ['refined',source+'/final-refresh.full.config.json'],
  ['coarse','runs/full-robot/learning/fast-motor/config.json'],
]) {
  const config=await read(input);
  assert.equal(config.implicit.newton.refresh_before_iteration_limit,true);
  assert.equal(config.implicit.auxiliary_rate_unknowns,true);
  assert.equal(config.implicit.condense_auxiliary,undefined);
  config.implicit.condense_auxiliary=true;
  const b=JSON.stringify(config);
  await writeFile(`${directory}/${name}.config.json`,b);
  outputs[`${name}.config.json`]=hash(b);
  if(name==='base')for(const condensed of [false,true]) {
    const diagnostic=structuredClone(config);
    diagnostic.steps=120;
    diagnostic.report_every=1;
    if(!condensed)delete diagnostic.implicit.condense_auxiliary;
    const filename=`diagnostic.${condensed?'condensed':'reference'}.config.json`;
    const encoded=JSON.stringify(diagnostic);
    await writeFile(`${directory}/${filename}`,encoded);
    outputs[filename]=hash(encoded);
  }
}
const scene='runs/full-robot/learning/point-feedback/scene.json';
const model=await read(scene);
assert.equal(model.options.motor_dynamics??'detailed','detailed');
inputs[import.meta.filename]=hash(await readFile(import.meta.filename));
await writeFile(directory+'/manifest.json',JSON.stringify({
  scope:'Experimental nonlinear elimination of the complete auxiliary block. Same detailed motor equations, CAD, world, policy, timestep and outer tolerances as the corresponding reference. Inner solves use 100x tighter tolerances and verify original auxiliary residuals against the outer absolute floor. No promoted speedup or accuracy claim.',
  scene,inputs,outputs,
  diagnostic_scope:'The 0.03 s prefixes deliberately end before the motion program finishes; they are event/subdivision diagnostics, never complete-motion acceptance cases.',
  comparisons:{base:source+'/final-refresh.base.execution.json',refined:source+'/final-refresh.full.execution.json',coarse:'runs/full-robot/learning/fast-motor/detailed.execution.json'},
},null,2));
console.log(`Prepared ${directory}; detailed scene ${scene}`);
