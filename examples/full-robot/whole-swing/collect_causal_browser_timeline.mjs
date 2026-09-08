import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',run='runs/interactive/causal-response';
const read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify=s=>assert.equal(source(s.path).sha256,s.sha256,s.path);
const statusPath=`${root}/causal-response-browser-status.json`,status=read(statusPath);
assert(status.complete&&status.cases.length===1);status.sources.forEach(verify);
const c=status.cases[0];c.sources.forEach(verify);
assert(c.measurement.completed&&c.measurement.validation_passed&&c.measurement.keyboard_commands_recorded);
assert(status.parity.passed&&status.parity.replay_exact&&status.parity.reset_exact);
const nativePath=`${root}/direct-support-status.json`,native=read(nativePath).cases.find(c=>c.name==='direct-steering');
assert(native.completed&&native.passed);native.sources.forEach(verify);
const capture=native.sources.find(s=>s.path.endsWith('.native.json'));
const recordingPath=`${run}/direct-turn.recording.json`,recording=read(recordingPath);
assert.equal(recording.error,null);assert.deepEqual(recording.runtime,read(capture.path).recording);
const timelinePath=`${run}/direct-turn.frames.json`,timeline=read(timelinePath);
const timingPath=`${run}/direct-turn.timing.json`,timing=read(timingPath);
assert.deepEqual(timeline.commands,c.measurement.performance.command_response.commands);
assert.deepEqual(timeline.received,timing.map(s=>({time_s:s.time_s,received_at_ms:s.received_at_ms})));
assert.equal(timeline.received.length,1200);assert(timeline.drawn.length>0);
const received=new Map(timeline.received.map(s=>[s.time_s,s.received_at_ms]));
for(const f of timeline.drawn)if(f.time_s>0)assert(f.submitted_at_ms>=received.get(f.time_s));
writeFileSync(`${root}/causal-response-browser-timeline-integrity.json`,JSON.stringify({version:1,passed:true,
  exact_native_recipe_seed_and_inputs:true,received_frames:timeline.received.length,
  distinct_drawn_frames:timeline.drawn.length,commands:timeline.commands.length,
  causal_physical_response_measured:false,
  sources:[statusPath,nativePath,capture.path,recordingPath,timelinePath,timingPath,import.meta.filename].map(source),
  scope:'The complete browser recording matches accepted native steering inputs. Dispatch, receipt and draw submission share one page clock, with draw timestamps after receipt of the same physics sample. Paired counterfactual physical trajectories are still required to measure causal response; monitor presentation and hardware latency are not measured.'},null,2)+'\n');
console.log({passed:true,received:timeline.received.length,drawn:timeline.drawn.length,commands:timeline.commands.length});
