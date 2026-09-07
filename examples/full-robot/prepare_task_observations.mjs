// Attach explicit teacher observations without modifying robot/world/policy code.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve,join} from 'node:path';
const root=resolve(import.meta.dirname,'../..');
const out=resolve(process.argv[2]||join(root,'runs/full-robot/learning/task-observations'));
const inputs={};
async function read(path){const b=await readFile(join(root,path));inputs[path]=createHash('sha256').update(b).digest('hex');return b;}
const scene=JSON.parse(await read('runs/full-robot/learning/joint-feedback/gain-0.5.scene.json'));
const config=JSON.parse(await read('runs/full-robot/learning/joint-feedback/gain-0.5.config.json'));
config.policy.task_observations=JSON.parse(await read('examples/full-robot/task-observations.json'));
await read('examples/full-robot/prepare_task_observations.mjs');
await mkdir(out,{recursive:true});const outputs={};
for(const [name,value] of [['scene.json',scene],['config.json',config]]){const b=JSON.stringify(value);await writeFile(join(out,name),b);outputs[name]=createHash('sha256').update(b).digest('hex');}
await writeFile(join(out,'manifest.json'),JSON.stringify({inputs,outputs,scope:'Additional named ideal body/foot observations. Same captured robot, world, reference, controller and initial condition as gain 0.5. No hardware sensing claim.'},null,2));
console.log(`Prepared task observations in ${out}`);
