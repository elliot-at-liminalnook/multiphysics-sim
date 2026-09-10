// Reusable experiment orchestration. Rust owns trajectory transforms, physics,
// motion fitting and prediction; this module transports and checks their data.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import {spawn} from 'node:child_process';
import {materialize} from './run_affine_speed_search.mjs';

export const read = path => JSON.parse(fs.readFileSync(path));
export const write = (path, value) => fs.writeFileSync(path, JSON.stringify(value)+'\n', {flag:'wx'});
export const pin = path => ({path, sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});

export async function execute(root, name, binary, args, stdoutName = name+'.stdout.log') {
  const out = fs.openSync(root+'/'+stdoutName, 'wx');
  const err = fs.openSync(root+'/'+name+'.stderr.log', 'wx');
  const start = Date.now();
  const result = await new Promise(resolve => {
    const child = spawn(binary, args, {stdio:['ignore',out,err], env:{...process.env,
      OMP_NUM_THREADS:'1', VECLIB_MAXIMUM_THREADS:'1', RAYON_NUM_THREADS:'2', EGOBOX_LOG:'off'}});
    write(root+'/'+name+'.launch.json', {binary,args,pid:child.pid??null,started_utc:new Date(start).toISOString()});
    child.once('error', error => resolve({exit_code:null,signal:null,error:error.message}));
    child.once('close', (exit_code,signal) => resolve({exit_code,signal,error:null}));
  });
  fs.closeSync(out); fs.closeSync(err);
  write(root+'/'+name+'.execution.json', {...result,wall_s:(Date.now()-start)/1000});
  return result;
}

export function validateScreen(source, screen) {
  const stride = source.task.period_s/source.config.step_s;
  const prefixSteps = screen.prefix_s/source.config.step_s;
  assert(Number.isSafeInteger(stride) && stride > 0, 'controller period must lie on physics grid');
  assert(Number.isSafeInteger(prefixSteps) && prefixSteps > 0 && prefixSteps % stride === 0,
    'prefix must end on controller grid');
  assert(screen.prefix_s < source.duration, 'prefix must precede declared full episode');
  assert(Number.isSafeInteger(screen.seed) && screen.seed >= 0);
  assert(Array.isArray(screen.fit_windows) && screen.fit_windows.length > 0);
  for (const window of screen.fit_windows) {
    assert(window.length === 2 && window.every(Number.isFinite));
    const [start,end] = window;
    assert(start >= 0 && end === screen.prefix_s && end-start >= source.task.period_s,
      'each fit must end at the prefix and contain at least two observations');
  }
  return {stride,prefixSteps};
}

export async function evaluatePlanarCandidate({sourceSpec,source,template,screen,predictor,path,values,ordinal}) {
  const {stride,prefixSteps} = validateScreen(source,screen);
  materialize(sourceSpec,source,values,path);
  const runtime = structuredClone(template);
  runtime.scene = read(path+'/scene.json');
  runtime.config = source.config;
  runtime.seed = screen.seed;
  runtime.completed_steps = prefixSteps;
  runtime.input_events = read(path+'/actions.json').slice(0,prefixSteps/stride)
    .map((values,k) => ({at_step:k*stride,values}));
  write(path+'/replay-input.json', {version:1,kind:'sampled_environment_recording',task:source.task,runtime,error:null});
  write(path+'/screen-inputs.json', {version:1,files:[path+'/replay-input.json',screen.runtime,predictor].map(pin)});
  const execution = await execute(path,'capture',screen.runtime,['--replay',path+'/replay-input.json'],'native.json');
  let capture = null;
  try { capture = read(path+'/native.json'); } catch {}
  const last = capture?.transitions?.at(-1);
  const valid = execution.exit_code === 0 && capture?.error === null && capture.requested_steps_completed
    && last?.time_s === screen.prefix_s && !last.terminated && !last.speed.fallen;
  if (!valid) {
    const error = capture?.error ?? execution.error ?? last?.termination_reasons ?? 'prefix did not complete';
    return {row:{ordinal,values,status:'failed',elapsed_s:last?.time_s??null,
      fallen:last?.speed?.fallen??null,error,predicted_speed_m_s:null},
      outcome:{status:'failed',reason:typeof error === 'string' ? error : JSON.stringify(error)},fits:[]};
  }
  // Certification is separate from process success; check the entire sampled prefix.
  assert.equal(last.completed_steps,prefixSteps);
  assert.equal(capture.frames.length,prefixSteps/stride+1);
  assert.equal(capture.transitions.length,prefixSteps/stride+1);
  assert(capture.transitions.every(t => !t.speed.fallen));
  assert(isDeepStrictEqual(capture.recording.scene,runtime.scene), 'runtime changed scene');
  assert(isDeepStrictEqual(capture.recording.input_events,runtime.input_events), 'runtime changed commands');
  assert.equal(capture.recording.config.step_s,source.config.step_s);
  assert.equal(capture.recording.seed,screen.seed);
  const poses = capture.frames.map(f => {
    const p = f.poses.find(p => p.name === screen.body_link); assert(p);
    return {time_s:f.time_s,pose:[...p.position_m.slice(0,2),Math.atan2(p.rotation[1][0],p.rotation[0][0])]};
  });
  const fits = [];
  for (let j=0;j<screen.fit_windows.length;j++) {
    const [start,end] = screen.fit_windows[j];
    const request = {samples:poses.filter(p => p.time_s >= start && p.time_s <= end),
      origin:poses[0],queries:[{time_s:source.duration}],heading_trend:true};
    write(path+`/fit-${j}.request.json`,request);
    assert.equal((await execute(path,'fit-'+j,predictor,
      [path+`/fit-${j}.request.json`,path+`/fit-${j}.result.json`])).exit_code,0);
    fits.push({request,result:read(path+`/fit-${j}.result.json`)});
  }
  const speeds = fits.map(f => f.result.forecasts[0].predicted_net_speed_m_s);
  assert(speeds.every(Number.isFinite));
  const predicted_speed_m_s = Math.max(...speeds);
  const row = {ordinal,values,status:'predicted',elapsed_s:screen.prefix_s,fallen:false,
    prefix_net_speed_m_s:last.speed.net_distance_m/screen.prefix_s,predicted_speed_m_s,
    fit_window_speeds_m_s:speeds,physical_full_horizon_qualified:false};
  write(path+'/prefix-check.json', {version:1,passed:true,steps:prefixSteps,
    frames:capture.frames.length,actions:runtime.input_events.length,
    scope:'All requested prefix steps, exact scene and input events, seed, timestep and zero sampled falls verified. Full-horizon performance remains unqualified.'});
  return {row,outcome:{status:'complete',objective:-predicted_speed_m_s,residuals:[-1]},fits};
}

// Bind Rust fit outputs to typed response slots. Shared slots must agree exactly;
// no fitted dynamics or forecast objective is recomputed here.
export function bindPlanarResponses(fits, windows, responseCount) {
  assert.equal(fits.length,windows.length);
  const responses = Array(responseCount).fill(null);
  fits.forEach(({request,result},j) => {
    const pose = result.heading_trend.anchor.pose;
    const values = [pose[0]-request.origin.pose[0],pose[1]-request.origin.pose[1],pose[2],...result.model.twist];
    const indices = [...windows[j].pose,...windows[j].twist];
    assert.equal(indices.length,6); assert.equal(values.length,6);
    indices.forEach((index,k) => {
      assert(Number.isSafeInteger(index) && index >= 0 && index < responseCount);
      assert(Number.isFinite(values[k]));
      if (responses[index] !== null) assert.equal(responses[index],values[k], 'shared response differs');
      responses[index] = values[k];
    });
  });
  assert(responses.every(Number.isFinite), 'all response slots must be bound');
  return responses;
}
