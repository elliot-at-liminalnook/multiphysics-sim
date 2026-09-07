// Prove this task change leaves executed physics and policy telemetry unchanged.
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [oldPath,newPath,output]=process.argv.slice(2);
assert(oldPath&&newPath&&output,'usage: check_scoring.mjs old-capture new-capture report');
const read=p=>JSON.parse(readFileSync(p)),old=read(oldPath),next=read(newPath);
const hash=v=>createHash('sha256').update(JSON.stringify(v)).digest('hex');
assert(old.completed&&next.completed&&!old.error&&!next.error);
assert.deepEqual(old.recording.config,next.recording.config);
assert.deepEqual(old.recording.scene,next.recording.scene);
assert.deepEqual(old.recording.input_events,next.recording.input_events);
const task=structuredClone(next.task);delete task.walking.heading;
assert.deepEqual(task,old.task);
const physical=run=>run.frames.map(f=>{const copy={...f};delete copy.stepping_wall_s;return copy;});
const physicalHash=hash(physical(old));assert.equal(hash(physical(next)),physicalHash);
assert.equal(old.transitions.length,next.transitions.length);
let headingReward=0,totalChange=0;
for(let i=0;i<old.transitions.length;i++){
 const a=old.transitions[i],b=next.transitions[i];
 assert.deepEqual(a.observations,b.observations);
 const heading=b.walking?.heading?.reward??0;
 assert.equal(b.reward_terms.find(r=>r.name==='walking.heading_tracking')?.value??0,heading);
 const stripped=structuredClone(b);delete stripped.walking.heading;
 stripped.reward_terms=stripped.reward_terms.filter(r=>r.name!=='walking.heading_tracking');
 assert(Math.abs((b.reward-a.reward)-heading)<1e-12);
 stripped.reward=a.reward;assert.deepEqual(stripped,a);
 headingReward+=heading;totalChange+=b.reward-a.reward;
}
assert(Math.abs(headingReward-totalChange)<1e-10);
const report={passed:true,frames:next.frames.length,physical_frame_hash:physicalHash,
 heading_reward:headingReward,total_reward_change:totalChange,
 final_heading:next.transitions.at(-1).walking.heading,
 sources:[oldPath,newPath].map(path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')})),
 scope:'Exact physical frames and actor telemetry after removing host wall timing. Only the declared heading reward and task telemetry change.'};
writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({passed:true,frames:report.frames}));
