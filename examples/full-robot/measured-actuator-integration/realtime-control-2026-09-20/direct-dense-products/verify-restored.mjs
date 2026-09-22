import fs from 'node:fs';
import assert from 'node:assert/strict';
const dir=import.meta.dirname,read=name=>JSON.parse(fs.readFileSync(`${dir}/${name}.json`));
const a=read('before.native'),b=read('restored.native');
assert(a.completed&&b.completed&&!a.error&&!b.error);assert.equal(a.frames.length,b.frames.length);
const fields=Object.keys(a.frames[0]).filter(k=>k!=='stepping_wall_s');
for(let i=0;i<a.frames.length;i++){assert.deepEqual(Object.keys(a.frames[i]),Object.keys(b.frames[i]));for(const field of fields)assert.deepEqual(a.frames[i][field],b.frames[i][field],`frame ${i}: ${field}`);}
assert.deepEqual(a.transitions,b.transitions);
assert.equal(read('restored.binary').sha256,read('disposition').restored_binary_sha256);
fs.writeFileSync(`${dir}/restored-parity.json`,JSON.stringify({completed:true,frames:a.frames.length,exact_fields:fields,all_task_transitions_exact:true,binary_identity_verified:true,wall_s:b.wall_s,scope:'Restoration verification after the unsuccessful experiment; timing is a single run, not a new optimization claim.'},null,2)+'\n');
console.log('Restored source replay: all 151 physical frames and task transitions exactly match the preserved binary');
