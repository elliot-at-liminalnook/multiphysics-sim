// Start only after other assistant builds/simulations/browser tests finish.
// Both binaries run exactly the same configuration. ABBA reduces order bias.
import {readFile,writeFile,mkdir,open} from 'node:fs/promises';
import {spawn,execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {cpus,platform,arch,release} from 'node:os';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/contact-history/benchmark';
const runners={previous:'runs/interactive/endpoint-correction/native-runner',optimized:'runs/interactive/contact-history/native-runner'};
const scene='runs/full-robot/learning/point-feedback/scene.json';
const config='runs/full-robot/learning/contact-history/base.config.json';
const reference='runs/full-robot/learning/endpoint-correction/base.execution.json';
const parent=JSON.parse(await readFile(reference));
const hash=async p=>createHash('sha256').update(await readFile(p)).digest('hex');
await mkdir(directory,{recursive:true});
const manifest={scope:'Sequential ABBA on identical inputs with frozen before/after binaries. No other assistant-launched heavy jobs should overlap. Host background activity is not controlled.',hardware:{platform:platform(),arch:arch(),release:release(),cpu:cpus()[0]?.model,logical_cpus:cpus().length},inputs:{},runs:[]};
for(const p of [scene,config,reference,...Object.values(runners),import.meta.filename])manifest.inputs[p]=await hash(p);
await writeFile(directory+'/processes-before.txt',execFileSync('ps',['-axo','pid,etime,pcpu,command']));
for(const [i,mode] of ['previous','optimized','optimized','previous'].entries()){
 const output=`${directory}/${i}-${mode}.execution.json`;
 const fd=await open(output,'w');console.log(`Starting ${i+1}/4 ${mode}`);
 try{
  const code=await new Promise((resolve,reject)=>{const p=spawn(runners[mode],[scene,config],{stdio:['ignore',fd.fd,'inherit']});p.once('error',reject);p.once('exit',resolve);});
  assert.equal(code,0);
 }finally{await fd.close();}
 const d=JSON.parse(await readFile(output));assert(d.completed&&d.error===null);
 for(const key of ['frames','terminal_frame','hybrid_steps','hybrid_solves','contact_steps','contact_impulses']){
  // Compare whole authoritative sections, never wall-time fields.
  assert(key in parent&&key in d,`Missing ${key}`);
  assert(isDeepStrictEqual(d[key],parent[key]),`Nonidentical ${key} in ${mode}`);
 }
 manifest.runs.push({order:i,mode,output,sha256:await hash(output),wall_s:d.stepping_wall_s,simulated_s:d.simulated_s});
 await writeFile(directory+'/manifest.json',JSON.stringify(manifest,null,2));console.log(JSON.stringify(manifest.runs.at(-1)));
}
const mean=mode=>manifest.runs.filter(x=>x.mode===mode).reduce((n,x)=>n+x.wall_s,0)/2;
manifest.result={previous_mean_s:mean('previous'),optimized_mean_s:mean('optimized'),speedup:mean('previous')/mean('optimized'),simulated_seconds_per_wall_second:2.8/mean('optimized')};
await writeFile(directory+'/manifest.json',JSON.stringify(manifest,null,2));console.log(JSON.stringify(manifest.result));
