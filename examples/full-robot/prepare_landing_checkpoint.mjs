// Compose an explicit support-qualified motion experiment; all control is Rust.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve,join} from 'node:path';
const root=resolve(import.meta.dirname,'../..');
const out=resolve(process.argv[2]||join(root,'runs/full-robot/learning/landing-checkpoint'));
const inputs={};
async function read(path){const b=await readFile(join(root,path));inputs[path]=createHash('sha256').update(b).digest('hex');return b;}
const scene=JSON.parse(await read('runs/full-robot/learning/task-observations/scene.json'));
const config=JSON.parse(await read('runs/full-robot/learning/task-observations/config.json'));
config.motion_gate=JSON.parse(await read('examples/full-robot/landing-checkpoint.json'));
config.steps=8000; // 2 s physics horizon includes up to 0.3 s waiting and final hold.
await read('examples/full-robot/prepare_landing_checkpoint.mjs');
await mkdir(out,{recursive:true});const outputs={};
const refined=structuredClone(config);refined.step_s/=2;refined.steps*=2;refined.report_every*=2;
for(const [name,value] of [['scene.json',scene],['config.json',config],['refined.config.json',refined]]){const b=JSON.stringify(value);await writeFile(join(out,name),b);outputs[name]=createHash('sha256').update(b).digest('hex');}
await writeFile(join(out,'manifest.json'),JSON.stringify({inputs,outputs,scope:'Reuse control.motion_clock to qualify four-foot support after planned descent. Ideal force observations and provisional dwell/force thresholds; not landing placement, balance or hardware acceptance.'},null,2));
console.log(`Prepared landing checkpoint in ${out}`);
