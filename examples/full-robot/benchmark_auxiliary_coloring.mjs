// Run only when other assistant-launched builds/browser/simulation jobs have
// finished. ABBA ordering reduces simple warmup/order bias; this is a local
// benchmark, not a claim that the host has no other activity.
import {readFile,writeFile,mkdir,open} from 'node:fs/promises';
import {spawn,execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {cpus,platform,arch,release} from 'node:os';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/auxiliary-coloring';
const runner='runs/interactive/auxiliary-coloring/native-runner';
const scene='runs/full-robot/learning/point-feedback/scene.json';
const configs={ordinary:'runs/full-robot/learning/auxiliary-condensation/base.config.json',colored:directory+'/base.config.json'};
const hash=async p=>createHash('sha256').update(await readFile(p)).digest('hex');
await mkdir(directory+'/benchmark',{recursive:true});
const results=[];
const manifest={scope:'Sequential ABBA native runs, identical detailed model/timestep/tolerances; only inner derivative coloring changes. No other assistant-launched builds/browser jobs should overlap. Host background activity is not controlled.',hardware:{platform:platform(),arch:arch(),release:release(),cpu:cpus()[0]?.model,logical_cpus:cpus().length},inputs:{},runs:results};
for(const p of [runner,scene,...Object.values(configs),import.meta.filename])manifest.inputs[p]=await hash(p);
await writeFile(directory+'/benchmark/processes-before.txt',execFileSync('ps',['-axo','pid,etime,pcpu,command']));
for(const [i,mode] of ['ordinary','colored','colored','ordinary'].entries()) {
  const output=`${directory}/benchmark/${i}-${mode}.execution.json`;
  const fd=await open(output,'w');const start=new Date().toISOString();
  console.log(`Starting ${i+1}/4: ${mode}`);
  try {
    const status=await new Promise((resolve,reject)=>{
      const child=spawn(runner,[scene,configs[mode]],{stdio:['ignore',fd.fd,'inherit']});
      child.once('error',reject);child.once('exit',(code,signal)=>resolve({code,signal}));
    });
    assert.equal(status.code,0,`${mode} run failed: ${JSON.stringify(status)}`);
  } finally {await fd.close();}
  const capture=JSON.parse(await readFile(output));
  assert(capture.completed&&capture.error===null);
  results.push({order:i,mode,output,sha256:await hash(output),start_utc:start,end_utc:new Date().toISOString(),simulated_s:capture.simulated_s,stepping_wall_s:capture.stepping_wall_s});
  await writeFile(directory+'/benchmark/manifest.json',JSON.stringify(manifest,null,2));
  console.log(JSON.stringify(results.at(-1)));
}
const mean=mode=>results.filter(x=>x.mode===mode).reduce((n,r)=>n+r.stepping_wall_s,0)/2;
manifest.result={ordinary_mean_wall_s:mean('ordinary'),colored_mean_wall_s:mean('colored'),ratio:mean('ordinary')/mean('colored'),colored_simulated_seconds_per_wall_second:2.8/mean('colored')};
await writeFile(directory+'/benchmark/manifest.json',JSON.stringify(manifest,null,2));
console.log(JSON.stringify(manifest.result));
