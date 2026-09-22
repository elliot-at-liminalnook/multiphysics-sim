import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const dir=import.meta.dirname,read=n=>JSON.parse(fs.readFileSync(`${dir}/${n}`));
const profile=read('cpu.cpuprofile'),nodes=new Map(profile.nodes.map(n=>[n.id,n])),parent=new Map(),self=new Map(),inclusive=new Map(),paths=new Map();
for(const node of profile.nodes)for(const child of node.children??[]){assert(!parent.has(child));parent.set(child,node.id);}
assert.equal(profile.samples.length,profile.timeDeltas.length);
const clean=s=>s.replace(/\[[0-9a-f]{16}\]/g,'');
let total=0;
for(let i=0;i<profile.samples.length;i++) {
 const dt=profile.timeDeltas[i],id=profile.samples[i];assert(dt>=0);total+=dt;
 const name=clean(nodes.get(id).callFrame.functionName);self.set(name,(self.get(name)??0)+dt);
 const ancestors=[],seen=new Set();let at=id;
 while(at!==undefined) {const key=clean(nodes.get(at).callFrame.functionName);ancestors.push(key);if(!seen.has(key)){inclusive.set(key,(inclusive.get(key)??0)+dt);seen.add(key);}at=parent.get(at);}
 if(name.includes('masked_kernel')) {const key=ancestors.slice(0,9).join('\n <- ');paths.set(key,(paths.get(key)??0)+dt);}
}
const ranked=map=>[...map].sort((a,b)=>b[1]-a[1]).map(([name,us])=>({name,seconds:us/1e6,percent_all_samples:100*us/total}));
const before=read('../force-output-storage/candidate.wasm.json'),current=read('cpu.wasm.json');assert(!before.error&&!current.error);assert.equal(before.frames.length,current.frames.length);
const fields=Object.keys(before.frames[0]).filter(k=>k!=='stepping_wall_s');
for(let i=0;i<before.frames.length;i++){assert.deepEqual(Object.keys(before.frames[i]),Object.keys(current.frames[i]));for(const key of fields)assert.deepEqual(before.frames[i][key],current.frames[i][key],`frame ${i}: ${key}`);}
const protocol=read('protocol.json');for(const [file,hash]of Object.entries(protocol.inputs))assert.equal(createHash('sha256').update(fs.readFileSync(`${dir}/${file}`)).digest('hex'),hash);
const report={scope:'1000 us requested Chrome CPU sampling after model load, including worker idle, message handling and serialization. Sample weights approximate time and inclusive rows overlap. This is not an allocation count or a throughput acceptance measurement.',samples:profile.samples.length,sampled_seconds:total/1e6,profile_duration_seconds:(profile.endTime-profile.startTime)/1e6,self:ranked(self),inclusive:ranked(inclusive),masked_gemm_stacks:ranked(paths),parity:{frames:before.frames.length,all_fields_except_wall_exact:true,fields,full_task_transitions_exact:true},input_hashes_verified:true};
fs.writeFileSync(`${dir}/analysis.json`,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({...report,self:report.self.slice(0,12),inclusive:report.inclusive.slice(0,12),masked_gemm_stacks:report.masked_gemm_stacks.slice(0,5)},null,2));
