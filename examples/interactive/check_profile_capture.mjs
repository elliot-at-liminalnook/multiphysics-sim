// Profiling may change timing, never the recorded experiment or physical frames.
import {readFileSync} from 'node:fs';
import assert from 'node:assert/strict';
const [reference,candidate]=process.argv.slice(2);assert(reference&&candidate);
const read=p=>JSON.parse(readFileSync(p)),a=read(reference),b=read(candidate);
assert(a.completed&&b.completed);assert.deepEqual(a.recording,b.recording);
assert.deepEqual(a.task,b.task);assert.deepEqual(a.contract,b.contract);
assert.deepEqual(a.transitions,b.transitions);assert.equal(a.frames.length,b.frames.length);
for(let i=0;i<a.frames.length;i++){
 delete a.frames[i].stepping_wall_s;delete b.frames[i].stepping_wall_s;
 assert.deepEqual(a.frames[i],b.frames[i],`profile changed frame ${i}`);
}
console.log(JSON.stringify({passed:true,frames:a.frames.length,scope:'Exact same-host frame, transition, contract, task and recording equality; only diagnostic wall timing excluded.'}));
