import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const root='runs/full-robot/learning/hip-timestep';
const source='runs/full-robot/learning/analytic-positions';
const inputs={},outputs={};
const hash=b=>createHash('sha256').update(b).digest('hex');
async function read(p){const b=await readFile(p);inputs[p]=hash(b);return JSON.parse(b);}
await mkdir(root,{recursive:true});
const parent=await read(source+'/manifest.json');
const original=await read(source+'/base.config.json');
const cases=[];
for(const [name,step] of [['250us',0.00025],['125us',0.000125],['62p5us',0.0000625]]){
 const config={...structuredClone(original),step_s:step,steps:Math.round(2.8/step),report_every:Math.round(0.01/step)};
 const p=`${root}/${name}.config.json`,b=JSON.stringify(config);await writeFile(p,b);outputs[p]=hash(b);
 cases.push({name,step_s:step,config:p,reference_capture:name==='250us'?source+'/base.execution.json':name==='125us'?source+'/refined.execution.json':null});
}
inputs[parent.scene]=hash(await readFile(parent.scene));inputs[import.meta.filename]=hash(await readFile(import.meta.filename));
await writeFile(root+'/manifest.json',JSON.stringify({scene:parent.scene,inputs,outputs,cases,window_s:[0.7,1.65],sample_period_s:0.001,
 scope:'Same model, policy and 2.8 s recipe. Observe the 0.7–1.65 s prefix window every 1 ms; do not claim completion of the full motion. Only nominal timestep and corresponding counts differ. No physical parameters or solver tolerances are changed.'},null,2));
console.log('Prepared three timestep traces.');
