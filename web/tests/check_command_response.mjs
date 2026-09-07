// Verify drawn-reference diagnostics against the exact native command replay.
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual as equal} from 'node:util';
import assert from 'node:assert/strict';
const [reportPath,recordingPath,nativePath,output]=process.argv.slice(2);
assert(output,'usage: check_command_response.mjs browser-report recording native-capture report');
const read=p=>JSON.parse(readFileSync(p)),hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const browser=read(reportPath),recording=read(recordingPath),native=read(nativePath);
assert(browser.completed&&native.completed&&!native.error);
assert(equal(recording.runtime,native.recording),'native replay must use the exact recorded scene, config, seed and inputs');
assert(equal(recording.task,native.task));
const response=browser.performance.command_response;
assert(response.drawn_reference_supported&&response.commands.length>0);
const channels=recording.runtime.scene.controller.inputs;
const indices=recording.runtime.config.policy.step_reference.command_channels.map(name=>channels.findIndex(c=>c.name===name));
assert(indices.every(i=>i>=0));
const same=(a,b)=>a?.length===b.length&&a.every((v,i)=>Math.abs(v-b[i])<1e-12);
for(const command of response.commands){
 assert(!command.superseded);
 const event=recording.runtime.input_events.find(e=>Math.abs(e.at_step*recording.runtime.config.step_s-command.issued_at_simulation_s)<1e-9);
 assert(event&&same(indices.map(i=>event.values[i]),command.requested_twist),'each keyboard request must be recorded at its measured simulation time');
 const first=native.frames.find(f=>f.time_s>command.issued_at_simulation_s&&f.policy?.step_reference?.reference.sample>command.previous_reference_sample
  &&same(f.policy.step_reference.reference.latched_twist,command.requested_twist));
 assert(first,'native execution never accepts the requested reference');
 assert.equal(command.reference_frame_time_s,first.time_s,'first received reference must match independent native execution');
 const drawn=native.frames.find(f=>f.time_s===command.drawn_frame_time_s);
 assert(drawn&&same(drawn.policy?.step_reference?.reference.latched_twist,command.requested_twist));
 assert(command.drawn_frame_time_s>=first.time_s);
 assert(Number.isFinite(command.reference_response_s)&&command.reference_response_s>=0);
 assert(Number.isFinite(command.drawn_reference_s)&&command.drawn_reference_s>=command.reference_response_s);
}
const result={version:1,passed:true,commands:response.commands,
 sources:[reportPath,recordingPath,nativePath].map(path=>({path,sha256:hash(path)})),
 scope:'Every keyboard request matches recorded inputs and the first updated walking-reference frame in an independent native execution. The drawn snapshot carries that reference at its reported simulation time. This validates measurement association, not monitor presentation, causal physical response, acceptable latency or realtime throughput.'};
writeFileSync(output,JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({passed:true,commands:result.commands.length}));
