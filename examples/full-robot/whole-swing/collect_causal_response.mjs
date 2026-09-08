import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {captureOutcome} from '../../interactive/capture_outcome.mjs';
import {pairedBodySignal,firstSustainedResponse,mapResponseToBrowser} from '../../interactive/causal_response.mjs';
const root='examples/full-robot/whole-swing',read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify=s=>assert.equal(source(s.path).sha256,s.sha256,s.path);
const planPath=`${root}/causal-response-plan.json`,plan=read(planPath);
const statusPath=`${root}/causal-response-status.json`,status=read(statusPath);
const integrityPath=`${root}/causal-response-integrity.json`,integrity=read(integrityPath);
assert(status.complete&&integrity.passed);for(const r of [plan,status,integrity])r.sources.forEach(verify);
const native= c=>{c.sources.forEach(verify);const s=c.sources.find(s=>s.path.endsWith('.native.json'));return [s,read(s.path)];};
const [commandedSource,commanded]=native(status.cases[0]);assert(status.cases[0].name==='commanded');
assert(commanded.completed&&status.cases[0].passed);
verify(plan.baseline);const original=read(plan.baseline.path);
const physical=f=>{const {stepping_wall_s,...rest}=f;return rest;};
assert.deepEqual(commanded.frames.map(physical),original.frames.map(physical));
for(const key of ['transitions','recording','contract','task'])assert.deepEqual(commanded[key],original[key]);
const browserPath=`${root}/causal-response-browser-status.json`,browser=read(browserPath);
browser.sources.forEach(verify);browser.cases[0].sources.forEach(verify);
const timelineIntegrityPath=`${root}/causal-response-browser-timeline-integrity.json`,timelineIntegrity=read(timelineIntegrityPath);
assert(timelineIntegrity.passed);timelineIntegrity.sources.forEach(verify);
const timelinePath='runs/interactive/causal-response/direct-turn.frames.json',timeline=read(timelinePath);
const recordingPath='runs/interactive/causal-response/direct-turn.recording.json';
assert.deepEqual(read(recordingPath).runtime,commanded.recording);
const originalActions=read(plan.cases[0].actions),period=commanded.task.period_s;
const commandIndices=commanded.recording.config.policy.step_reference.command_channels.map(name=>{
  const i=commanded.recording.scene.controller.inputs.findIndex(c=>c.name===name);assert(i>=0);return i;
});
const cases=plan.probes.map(probe=>{
  const result=status.cases.find(c=>c.name===probe.name),def=plan.cases.find(c=>c.name===probe.name);
  const [capture,r]=native(result);captureOutcome(r);
  for(const key of ['scene','config','seed'])assert.deepEqual(r.recording[key],commanded.recording[key]);
  assert.deepEqual(r.task,commanded.task);assert.deepEqual(r.contract,commanded.contract);
  const actions=read(def.actions),begin=Math.round(probe.start_s/period),end=Math.round(probe.end_s/period);
  const held=begin?originalActions[begin-1]:commanded.recording.scene.controller.inputs.map(c=>c.initial);
  assert.equal(actions.length,originalActions.length);
  actions.forEach((a,i)=>assert.deepEqual(a,i>=begin&&i<end?held:originalActions[i]));
  actions.forEach((a,i)=>a.forEach((v,j)=>{if(!commandIndices.includes(j))assert.equal(v,originalActions[i][j]);}));
  const command=timeline.commands.find(c=>c.stage===probe.stage);assert(command);
  assert.equal(command.issued_at_simulation_s,probe.start_s);
  assert.deepEqual(command.requested_twist,commandIndices.map(i=>originalActions[begin][i]));
  const n=Math.min(r.frames.length,commanded.frames.length);
  for(let i=0;i<n;i++){
    assert(Math.abs(r.frames[i].time_s-i*period)<1e-10);
    assert(Math.abs(commanded.frames[i].time_s-i*period)<1e-10);
  }
  for(let i=0;i<n&&r.frames[i].time_s<=probe.start_s;i++)assert.deepEqual(physical(r.frames[i]),physical(commanded.frames[i]));
  const reached=r.frames.at(-1).time_s>=probe.start_s;
  let signal=[],response=null,anyResponse=null,mapped=null;
  if(reached){
    signal=pairedBodySignal(commanded.frames.slice(0,n),r.frames.slice(0,n),probe);
    response=firstSustainedResponse(signal,probe);
    anyResponse=firstSustainedResponse(signal.map(s=>({...s,value:probe.metric==='yaw'?Math.abs(s.yaw_difference_rad):s.horizontal_difference_m})),probe);
    mapped=mapResponseToBrowser(response,timeline,probe.stage,signal,probe.threshold);
  }
  const window=signal.filter(s=>s.time_s>=probe.start_s&&s.time_s<=probe.end_s);
  const signalPath=`${root}/causal-response-${probe.name}-signal.json`;
  writeFileSync(signalPath,JSON.stringify({version:1,probe,samples:window})+'\n');
  return {name:probe.name,probe,counterfactual_completed:r.completed,counterfactual_task_passed:result.passed,
    counterfactual_acceptance:result.acceptance,counterfactual_error:r.error,
    exact_physical_prefix:true,only_declared_input_interval_changed:true,
    complete_response_window:r.frames.at(-1).time_s>=probe.end_s,
    sampled_directed_response:response,sampled_any_direction_response:anyResponse,
    browser_association:mapped,
    maximum_directed_difference:window.length?Math.max(...window.map(s=>s.value)):null,
    minimum_directed_difference:window.length?Math.min(...window.map(s=>s.value)):null,
    capture,signal:source(signalPath)};
});
writeFileSync(`${root}/causal-response-summary.json`,JSON.stringify({version:1,
  commanded_task_passed:true,commanded_full_repeat_exact:true,commanded_capture:commandedSource,
  cases,browser_performance:browser.cases[0].measurement.performance.active_motion,
  sources:[planPath,statusPath,integrityPath,browserPath,timelineIntegrityPath,timelinePath,recordingPath,
    import.meta.filename,'examples/interactive/causal_response.mjs','examples/interactive/capture_outcome.mjs'].map(source),
  scope:'Directed body-pose response in paired native simulations with exact pre-command physical states and one changed input interval. Native commanded trajectory is associated with the exact recorded browser recipe and its receipt/draw timeline. Dwell is sampled, not continuous; detection is not settling or completed reversal. Counterfactual WASM portability, monitor presentation, hardware response and direct-gait timestep accuracy are unmeasured. No controller qualification or realtime acceptance is inferred.'},null,2)+'\n');
console.log(JSON.stringify(cases.map(c=>({name:c.name,completed:c.counterfactual_completed,
  window_complete:c.complete_response_window,response:c.sampled_directed_response,
  any:c.sampled_any_direction_response,browser:c.browser_association})),null,2));
