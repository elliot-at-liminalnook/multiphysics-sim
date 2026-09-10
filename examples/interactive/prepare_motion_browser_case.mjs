// Package already completed Rust materializations/runs for isolated browser checks.
// No transforms, controller execution or physics are implemented in this host.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const [directory,bundle,presetPrefix]=process.argv.slice(2);
assert(directory&&bundle&&presetPrefix,'usage: prepare_motion_browser_case prepared-directory isolated-bundle preset-prefix');
const read=n=>JSON.parse(fs.readFileSync(path.join(directory,n+'.json')));
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v)+'\n',{flag:'wx'});
const base=read('base-native'),identity=read('identity-native'),changed=read('changed-native');
const clean=frame=>{const out=structuredClone(frame);delete out.stepping_wall_s;return out;};
assert([base,identity,changed].every(c=>c.completed&&c.error===null),'complete captures required');
assert(JSON.stringify(base.frames.map(clean))===JSON.stringify(identity.frames.map(clean)),'identity physical frames differ');
assert(JSON.stringify(base.transitions)===JSON.stringify(identity.transitions),'identity task outcomes differ');
assert(base.frames.length===changed.frames.length,'same horizon required');
const changedIndices=new Set();
for(let i=0;i<base.frames.length;i++)base.frames[i].joint_positions.forEach((v,j)=>{
 if(v!==changed.frames[i].joint_positions[j])changedIndices.add(j);
});
assert(changedIndices.size>0,'variant must affect physical joint motion');
const scene=read('scene'),actions=read('actions'),recipe=read('parameterization');
const catalogPath=path.join(bundle,'catalog.json'),catalog=JSON.parse(fs.readFileSync(catalogPath));
for(const candidate of ['identity','changed']){
 const expected=read(candidate+'-motion'),values=read(candidate+'-values'),native=read(candidate+'-native');
 assert(JSON.stringify(expected.variant.scene)===JSON.stringify(native.recording.scene),'native capture uses a different materialized scene');
 const preset=presetPrefix+'-'+candidate;
 assert(!catalog.presets.some(p=>p.id===preset),'preset already exists');
 write(path.join(directory,candidate+'-browser-case.json'),{request:{scene,actions,recipe,values},expected});
 const destination='data/'+preset+'.json';
 write(path.join(bundle,destination),{scene:expected.variant.scene,config:native.recording.config,task:native.task});
 catalog.presets.push({id:preset,path:destination,task:true});
}
fs.writeFileSync(catalogPath,JSON.stringify(catalog,null,2)+'\n');
const report={identity_physics_and_task_exact:true,changed_joint_position_indices:[...changedIndices].sort((a,b)=>a-b),
 scope:'Short controller sensitivity and transport acceptance, not locomotion, optimality, learned accuracy or physical calibration.'};
write(path.join(directory,'browser-case-preparation.json'),report);
console.log(JSON.stringify(report));
