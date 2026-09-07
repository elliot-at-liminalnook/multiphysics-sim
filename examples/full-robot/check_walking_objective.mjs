// Prove that scoring changes neither physical execution nor actor observations.
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [originalPath,livePath,rescorePath,liftsPath,output]=process.argv.slice(2);
assert(output,'usage: check_walking_objective original-capture live-capture rescore lift-summary output');
const read=p=>JSON.parse(readFileSync(p)),hash=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const original=read(originalPath),live=read(livePath),rescore=read(rescorePath),lifts=read(liftsPath);
assert(original.completed&&live.completed);assert.deepEqual(live.recording,original.recording);
assert.equal(live.frames.length,original.frames.length);
for(let i=0;i<live.frames.length;i++){
 const {stepping_wall_s,...a}=live.frames[i],{stepping_wall_s:unused,...b}=original.frames[i];assert.deepEqual(a,b,`physics frame ${i}`);
 const {walking,reward,reward_terms,...x}=live.transitions[i],{reward:oldReward,reward_terms:oldTerms,...y}=original.transitions[i];
 assert.deepEqual(x,y,`actor/transition ${i}`);
 assert.deepEqual(reward_terms.filter(r=>!r.name.startsWith('walking.')),oldTerms);
 assert(Math.abs(reward-oldReward-walking.body_reward-walking.step_reward)<1e-12);
}
const outcomes=live.transitions.flatMap(t=>t.walking.outcome?[t.walking.outcome]:[]);
assert.deepEqual(outcomes,rescore.outcomes);assert.equal(outcomes.length,lifts.lifts.length);
for(const o of outcomes){const independent=lifts.lifts.find(l=>l.step===o.step);assert(independent);
 assert.equal(o.passed,independent.passed);assert.equal(o.foot,independent.foot);
 assert(Math.abs(o.lift.longest_qualifying_span_s-independent.longest_qualifying_span_s)<1e-12);
}
const total=live.transitions.reduce((n,t)=>n+t.reward,0);assert(Math.abs(total-rescore.total_reward)<1e-9);
const result={passed:true,sources:[originalPath,livePath,rescorePath,liftsPath].map(hash),frames:live.frames.length,
 physical_frames_exact:true,actor_observations_and_actions_exact:true,offline_lift_outcomes_exact:true,
 original_reward:original.transitions.reduce((n,t)=>n+t.reward,0),new_reward:total,
 walking:live.transitions.at(-1).walking,
 scope:'Task-only addition through the production environment; every original physical frame, actor observation, action and original reward term is unchanged. Online step outcomes match the independent offline window audit. This does not promote the unchanged controller.'};
writeFileSync(output,JSON.stringify(result,null,2)+'\n');console.log({passed:result.passed,frames:result.frames,old:result.original_reward,new:result.new_reward});
