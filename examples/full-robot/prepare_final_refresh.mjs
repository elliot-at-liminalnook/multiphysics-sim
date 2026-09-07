import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/sample-reuse';
const inputs={},outputs={};
async function read(path) {
  const b=await readFile(path);
  inputs[path]=createHash('sha256').update(b).digest('hex');
  return JSON.parse(b);
}
// Preserve effective solver defaults from the measured run, not a second set
// of invented tolerances in a preparation script.
const captured=await read(directory+'/newton-audit.execution.json');
assert.equal(captured.completed_steps,captured.requested_steps);
assert.equal(captured.simulated_s,1.52);
assert(captured.error.startsWith('simulation horizon ended before motion completed:'));
for(const [input,output] of [
  ['newton-audit.config.json','final-refresh.config.json'],
  ['config.json','final-refresh.base.config.json'],
  ['config.json','final-refresh.viewer.config.json'],
  ['refined.config.json','final-refresh.full.config.json']
]) {
  const config=await read(directory+'/'+input);
  config.implicit.newton=structuredClone(captured.implicit.newton);
  config.implicit.newton.refresh_before_iteration_limit=true;
  if(output==='final-refresh.viewer.config.json')config.profile_solver=false;
  const b=JSON.stringify(config);
  await writeFile(directory+'/'+output,b);
  outputs[output]=createHash('sha256').update(b).digest('hex');
}
await read('examples/full-robot/sample-reuse-experiment.json');
inputs['examples/full-robot/prepare_final_refresh.mjs']=createHash('sha256').update(await readFile(import.meta.filename)).digest('hex');
await writeFile(directory+'/final-refresh.manifest.json',JSON.stringify({
  scope:'Opt-in fresh matrices for the final two Newton iterations. Same effective tolerances, iteration cap, physical model, controller and servo-sample reuse. Diagnostic horizon is intentionally incomplete; base/full configs run the complete motion.',
  inputs,outputs
},null,2));
console.log('Prepared final-iteration refresh diagnostic and complete-motion configs.');
