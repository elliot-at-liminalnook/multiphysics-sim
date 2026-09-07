// Sequential ABBA benchmark for two frozen embedded-session runner binaries.
// Start only after other assistant-launched builds/simulations/browser tests end.
import {readFile,writeFile,mkdir,open} from 'node:fs/promises';
import {spawn,execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {cpus,platform,arch,release} from 'node:os';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const [directory,before,after,scene,config,reference,afterConfig,afterReference]=process.argv.slice(2);
assert(reference,'usage: benchmark_exact_runtime.mjs output-directory before-runner after-runner scene config reference-capture [after-config after-reference]');
assert(Boolean(afterConfig)===Boolean(afterReference),'after-config and after-reference must be supplied together');
const hash=async p=>createHash('sha256').update(await readFile(p)).digest('hex');
const parent=JSON.parse(await readFile(reference));assert(parent.completed&&parent.error===null);
const candidate=afterReference?JSON.parse(await readFile(afterReference)):parent;
assert(candidate.completed&&candidate.error===null);
assert.equal(candidate.simulated_s,parent.simulated_s);
const sections=['frames','terminal_frame','hybrid_steps','hybrid_solves','contact_steps','contact_impulses'];
const manifest={scope:'Frozen binaries, sequential ABBA, exact repeatability against each declared reference. Optional after-config/reference permits explicit algorithm experiments; cross-method physical equivalence requires a separate trajectory comparison. No other assistant-launched heavy jobs should overlap; host background activity is not controlled. Stepping time excludes process setup and capture serialization.',hardware:{platform:platform(),arch:arch(),release:release(),cpu:cpus()[0]?.model,logical_cpus:cpus().length},inputs:{},runs:[]};
await mkdir(directory,{recursive:true});
for(const p of [before,after,scene,config,reference,afterConfig,afterReference,import.meta.filename].filter(Boolean))manifest.inputs[p]=await hash(p);
await writeFile(directory+'/processes-before.txt',execFileSync('ps',['-axo','pid,etime,pcpu,command']));
for(const [i,mode] of ['before','after','after','before'].entries()){
 const output=`${directory}/${i}-${mode}.execution.json`;
 const fd=await open(output,'w');console.log(`Starting ${i+1}/4 ${mode}`);
 try{
  const code=await new Promise((resolve,reject)=>{const p=spawn(mode==='before'?before:after,[scene,mode==='after'?(afterConfig??config):config],{stdio:['ignore',fd.fd,'inherit']});p.once('error',reject);p.once('exit',resolve);});assert.equal(code,0);
 }finally{await fd.close();}
 const d=JSON.parse(await readFile(output));assert(d.completed&&d.error===null);
 const expected=mode==='after'?candidate:parent;
 for(const k of sections){assert(k in d&&k in expected,`Missing ${k}`);assert(isDeepStrictEqual(d[k],expected[k]),`Nonidentical ${k} in run ${i}`);}
 manifest.runs.push({order:i,mode,config:mode==='after'?(afterConfig??config):config,reference:mode==='after'?(afterReference??reference):reference,output,sha256:await hash(output),wall_s:d.stepping_wall_s,simulated_s:d.simulated_s});
 await writeFile(directory+'/manifest.json',JSON.stringify(manifest,null,2));console.log(JSON.stringify(manifest.runs.at(-1)));
}
const mean=mode=>manifest.runs.filter(x=>x.mode===mode).reduce((n,x)=>n+x.wall_s,0)/2;
manifest.result={before_mean_s:mean('before'),after_mean_s:mean('after'),speedup:mean('before')/mean('after'),simulated_seconds_per_wall_second:parent.simulated_s/mean('after')};
await writeFile(directory+'/manifest.json',JSON.stringify(manifest,null,2));console.log(JSON.stringify(manifest.result));
