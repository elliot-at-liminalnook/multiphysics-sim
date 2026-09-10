// Reduction of recorded physical motion and exact held controller state only.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {recordedContactMotion} from '../../interactive/recorded_contact_motion.mjs';
const [prefix]=process.argv.slice(2);assert(prefix,'capture prefix required');
const read=p=>JSON.parse(fs.readFileSync(p));
const capturePath=prefix+'.native.json',replayPath=prefix+'.policy-state.json';
const c=read(capturePath),r=read(replayPath);
assert(c.completed&&c.frames.at(-1).time_s===8&&r.maximum_command_error_rad===0);
assert.equal(r.source_capture,capturePath);
const states=new Map(r.samples.map(s=>[s.frame_time_s,s.state]));
const results=[];
for(const [leg,marker] of c.recording.config.policy.task_observations.markers.entries()){
 const index=c.recording.scene.robot.links.findIndex(l=>l.name===marker.link);
 const motion=c.frames.map(f=>recordedContactMotion(f,index,marker.link));
 const categories={steady_forward:0,steady_reverse:0,other:0},bins=Array(16).fill(0);
 for(let i=1;i<motion.length;i++){
  const a=motion[i-1],b=motion[i];if(a.force<1||b.force<1)continue;
  const path=(a.speed+b.speed)/2*(b.time_s-a.time_s);
  const key=a.time_s>=1.4&&b.time_s<=3?'steady_forward':
   a.time_s>=4.8&&b.time_s<=6.4?'steady_reverse':'other';
  categories[key]+=path;
  const state=states.get(b.time_s);assert(state);
  const p=c.recording.scene.controller.parameters,foot=p.motion.feet[leg];
  const phase=((state.phase/p.period_s-foot.phase_offset)%1+1)%1;
  bins[Math.min(15,Math.floor(phase*16))]+=path;
 }
 assert(Math.abs(Object.values(categories).reduce((a,b)=>a+b,0)-bins.reduce((a,b)=>a+b,0))<1e-12);
 results.push({link:marker.link,loaded_path_m:categories,command_phase_bins:bins});
}
console.log(JSON.stringify({source:capturePath,policy_replay_source:replayPath,
 inputs:[capturePath,replayPath].map(path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')})),
 results,scope:'Recorded load-weighted material speed integrated exactly as the existing eight-second screen. Phase bins refer to the controller state held for the physics interval, not an independently observed contact mode; other includes acceleration, braking and settling.'},null,2));
