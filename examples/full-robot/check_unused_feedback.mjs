import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [fullPath,reducedPath,output]=process.argv.slice(2);assert(output);
const read=p=>JSON.parse(readFileSync(p)),full=read(fullPath),reduced=read(reducedPath);
assert(full.completed&&reduced.completed);assert.equal(full.frames.length,reduced.frames.length);
let omitted=0;
for(let i=0;i<full.frames.length;i++)for(const [key,value] of Object.entries(full.frames[i])){
 if(key==='stepping_wall_s')continue;
 if(key==='policy')for(const [k,v] of Object.entries(value??{})){
  if(['body_feedback','point_feedback'].includes(k)){assert.equal(reduced.frames[i].policy[k],undefined);continue;}
  if(k==='observations'){
   for(const [name,x]of Object.entries(v)){
    if(/\.body_correction$|\.point_correction$|\.floor_force_world\./.test(name)){assert.equal(reduced.frames[i].policy.observations[name],undefined);omitted++;}
    else assert.equal(reduced.frames[i].policy.observations[name],x);
   }
  }else assert.deepEqual(reduced.frames[i].policy[k],v);
 }else assert.deepEqual(reduced.frames[i][key],value,`physical frame ${i}.${key}`);
}
assert(omitted>0);assert.deepEqual(reduced.transitions,full.transitions);
const hash=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
writeFileSync(output,JSON.stringify({version:1,passed:true,full:hash(fullPath),reduced:hash(reducedPath),exact_physical_frames:full.frames.length,omitted_observation_samples:omitted,scope:'Unused body/point feedback calculations and policy-side floor-force observations are omitted. Every physical frame, retained controller value and teacher transition remains exact. Upstream planner and physical contact still use ideal simulated state.'},null,2)+'\n');console.log(output);
