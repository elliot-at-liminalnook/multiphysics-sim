// Promote a completed physical prefix to its already-declared full episode.
// No controller, physical property, timestep, initial condition or seed changes.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import {pathToFileURL} from 'node:url';
import {read,write,pin,execute} from './planar_speed_experiment.mjs';

export function extendPrefix(prefix, actions) {
  assert.equal(prefix.version,1);
  assert.equal(prefix.kind,'sampled_environment_recording');
  assert.equal(prefix.error,null);
  const runtime = prefix.runtime, step = runtime.config.step_s;
  const stride = prefix.task.period_s/step, steps = runtime.config.steps;
  assert(Number.isSafeInteger(stride) && stride > 0);
  assert(Number.isSafeInteger(steps) && steps > runtime.completed_steps && steps % stride === 0);
  assert(runtime.completed_steps % stride === 0);
  assert(Math.abs(steps*step-runtime.scene.duration_s) <= 1e-10, 'declared scene/config duration mismatch');
  assert.equal(actions.length,steps/stride,'complete authored command sequence required');
  assert(actions.every(a => a.length === runtime.scene.controller.inputs.length && a.every(Number.isFinite)));
  const full = structuredClone(prefix);
  full.runtime.completed_steps = steps;
  full.runtime.input_events = actions.map((values,i) => ({at_step:i*stride,values}));
  assert(isDeepStrictEqual(full.runtime.input_events.slice(0,runtime.completed_steps/stride),runtime.input_events),
    'full command sequence does not reproduce the observed prefix');
  const restored = structuredClone(full);
  restored.runtime.completed_steps = runtime.completed_steps;
  restored.runtime.input_events = restored.runtime.input_events.slice(0,runtime.input_events.length);
  assert(isDeepStrictEqual(restored,prefix),'unexpected change beyond requested steps and command extension');
  return full;
}

export function verifyEpisode(capture, input, prefixCapture) {
  const runtime = input.runtime, duration = runtime.completed_steps*runtime.config.step_s;
  const last = capture.transitions?.at(-1);
  const complete = capture.error === null && capture.completed && capture.requested_steps_completed
    && last?.time_s === duration && !last.terminated && !last.speed.fallen;
  if (!complete) return {passed:false,status:'failed',elapsed_s:last?.time_s??null,
    fallen:last?.speed?.fallen??null,error:capture.error??last?.termination_reasons??'full episode did not complete',
    net_speed_m_s:null};
  const observations = duration/input.task.period_s+1;
  assert(Number.isSafeInteger(observations));
  assert.equal(capture.frames.length,observations); assert.equal(capture.transitions.length,observations);
  assert.equal(last.completed_steps,runtime.completed_steps);
  assert(capture.transitions.every(t => !t.speed.fallen),'sampled fall within episode');
  assert(isDeepStrictEqual(capture.recording.scene,runtime.scene),'scene differs');
  assert(isDeepStrictEqual(capture.recording.input_events,runtime.input_events),'command sequence differs');
  assert.equal(capture.recording.config.step_s,runtime.config.step_s);
  assert.equal(capture.recording.seed,runtime.seed);
  assert.equal(capture.recording.completed_steps,runtime.completed_steps);
  assert(prefixCapture.requested_steps_completed && prefixCapture.error === null);
  assert(prefixCapture.frames.length < capture.frames.length);
  for (let i=0;i<prefixCapture.frames.length;i++) {
    const a = {...capture.frames[i]}, b = {...prefixCapture.frames[i]};
    delete a.stepping_wall_s; delete b.stepping_wall_s;
    assert(isDeepStrictEqual(a,b),'physical prefix frame differs at '+i);
    assert(isDeepStrictEqual(capture.transitions[i],prefixCapture.transitions[i]),'prefix transition differs at '+i);
  }
  assert(Number.isFinite(last.speed.net_distance_m));
  return {version:1,passed:true,status:'complete',elapsed_s:duration,step_s:runtime.config.step_s,
    steps:runtime.completed_steps,actions:runtime.input_events.length,frames:capture.frames.length,
    prefix_parity_frames:prefixCapture.frames.length,fallen:false,net_distance_m:last.speed.net_distance_m,
    net_speed_m_s:last.speed.net_distance_m/duration,displacement_xy_m:last.speed.displacement_xy_m,
    scope:'Full declared episode, all steps/actions, zero sampled falls and exact original physical prefix parity excluding wall timing. This is a same-model speed measurement, not numerical convergence, hardware accuracy or a physical maximum.'};
}

async function main(specPath) {
  assert(specPath,'usage: run_prefix_candidate_episode specification.json');
  const spec = read(specPath); assert.equal(spec.version,1);
  const prefixRoot = spec.prefix_directory;
  assert.equal(read(prefixRoot+'/capture.execution.json').exit_code,0);
  assert(read(prefixRoot+'/prefix-check.json').passed);
  const prefix = read(prefixRoot+'/replay-input.json'), actions = read(prefixRoot+'/actions.json');
  const source = read(prefixRoot+'/summary.json'); assert.equal(source.status,'predicted');
  assert(isDeepStrictEqual(source.values,read(prefixRoot+'/values.json')),'selected parameter values differ');
  const full = extendPrefix(prefix,actions), root = spec.output_directory;
  const sourcePins = read(prefixRoot+'/screen-inputs.json').files;
  const runtimePin = sourcePins.find(p => p.path === spec.runtime); assert(runtimePin,'original runtime pin required');
  assert.equal(pin(spec.runtime).sha256,runtimePin.sha256,'runtime binary differs from physical prefix');
  const prefixPin = sourcePins.find(p => p.path === prefixRoot+'/replay-input.json'); assert(prefixPin);
  assert.equal(pin(prefixPin.path).sha256,prefixPin.sha256,'authored prefix changed after execution');
  for (const file of ['scene.json','actions.json']) {
    const expected = read(prefixRoot+'/inputs.json').files.find(p => p.path === prefixRoot+'/'+file); assert(expected);
    assert.equal(pin(expected.path).sha256,expected.sha256,'authored source changed: '+file);
  }
  fs.mkdirSync(root); write(root+'/spec.json',spec); write(root+'/replay-input.json',full);
  write(root+'/inputs.json',{version:1,files:[specPath,import.meta.filename,
    new URL('./planar_speed_experiment.mjs',import.meta.url).pathname,spec.runtime,
    prefixRoot+'/replay-input.json',prefixRoot+'/native.json',prefixRoot+'/summary.json',
    prefixRoot+'/prefix-check.json',prefixRoot+'/capture.execution.json',prefixRoot+'/screen-inputs.json',
    prefixRoot+'/inputs.json',prefixRoot+'/values.json',prefixRoot+'/actions.json',root+'/replay-input.json'].map(pin),
    scope:'Exact continuation of a verified physical prefix to its original full episode using the same pinned Rust runtime. Original observations are replayed from the same initial state; this does not splice or reset dynamics.'});
  const execution = await execute(root,'capture',spec.runtime,['--replay',root+'/replay-input.json'],'native.json');
  if (execution.exit_code !== 0) {
    write(root+'/summary.json',{version:1,status:'failed',net_speed_m_s:null,execution}); process.exitCode = 1; return;
  }
  const summary = verifyEpisode(read(root+'/native.json'),full,read(prefixRoot+'/native.json'));
  write(root+'/summary.json',{...summary,source_prefix:prefixRoot,values:source.values,
    prior_prefix_speed_m_s:source.prefix_net_speed_m_s,prior_forecasts_m_s:source.fit_window_speeds_m_s});
  console.log(JSON.stringify(summary));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main(process.argv[2]);
