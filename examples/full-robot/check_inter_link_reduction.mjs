// Compare a no-inter-link-force trajectory with its retained-force reference.
// Exact equality is appropriate only for test paths that never activate these forces.
import {readFileSync} from 'node:fs';
import assert from 'node:assert/strict';
const [reference,candidate]=process.argv.slice(2);assert(reference&&candidate);
const read=p=>JSON.parse(readFileSync(p)),a=read(reference),b=read(candidate);
assert(a.completed&&b.completed);
assert(!a.recording.scene.options.omit_inter_link_contact);
assert.equal(b.recording.scene.options.omit_inter_link_contact,true);
delete a.recording.scene.options.omit_inter_link_contact;
delete b.recording.scene.options.omit_inter_link_contact;
assert.deepEqual(a.recording,b.recording);assert.deepEqual(a.task,b.task);
assert.deepEqual(a.contract,b.contract);assert.deepEqual(a.transitions,b.transitions);
assert.equal(a.frames.length,b.frames.length);
for(let i=0;i<a.frames.length;i++){
 assert(a.frames[i].contacts.every(c=>c.other==null),'reference has active inter-link contact');
 delete a.frames[i].stepping_wall_s;delete b.frames[i].stepping_wall_s;
 assert.deepEqual(a.frames[i],b.frames[i],`physical/telemetry difference at frame ${i}`);
}
console.log(JSON.stringify({passed:true,frames:a.frames.length,
 scope:'Exact same-host physical/telemetry frames, task and input history. Only explicit omission of inter-link forces and wall timing differ. Independent geometric overlap audit must also pass.'}));
