// Compare only the inner Newton correction coordinates. No physical or raw
// equation-residual limits change. Prefixes intentionally stop mid-program.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/endpoint-correction';
const source='runs/full-robot/learning/auxiliary-coloring';
const inputs={},outputs={};
const hash=b=>createHash('sha256').update(b).digest('hex');
async function read(p){const b=await readFile(p);inputs[p]=hash(b);return JSON.parse(b);}
async function write(name,c){const b=JSON.stringify(c);const p=`${directory}/${name}.config.json`;await writeFile(p,b);outputs[p]=hash(b);}
await mkdir(directory,{recursive:true});
const previous=await read(source+'/manifest.json');
for(const name of ['base','refined']){
  const c=await read(`${source}/${name}.config.json`);
  assert(c.implicit.condense_auxiliary&&c.implicit.auxiliary_rate_unknowns&&c.implicit.color_auxiliary_jacobian);
  c.implicit.auxiliary_endpoint_correction_scale=true;
  await write(name,c);
  if(name==='refined')for(const mode of ['ordinary','endpoint']){
    const prefix=structuredClone(c);
    prefix.steps=Math.round(0.03/prefix.step_s);prefix.report_every=1;
    prefix.implicit.auxiliary_endpoint_correction_scale=mode==='endpoint';
    prefix.implicit.newton_audit_window_s=[0.021,0.023];
    await write('prefix.'+mode,prefix);
  }
}
inputs[import.meta.filename]=hash(await readFile(import.meta.filename));
await writeFile(directory+'/manifest.json',JSON.stringify({
  scene:previous.scene,inputs,outputs,
  scope:'Inner correction scale becomes (1+abs(old+h*rate))/h. Original residual tolerances, detailed motor laws, controller, contact, and outer Newton checks unchanged. Opt-in experiment; no accuracy or realtime promotion.',
  comparisons:{base:source+'/base.execution.json',refined:source+'/refined.execution.json'},
  prefix_expectation:'240 steps at 0.125 ms. Exit 1 only because the 2.4 s motion program is unfinished; inspect completed_steps and error rather than accepting arbitrary failures.'
},null,2));
console.log('Prepared endpoint correction experiment');
