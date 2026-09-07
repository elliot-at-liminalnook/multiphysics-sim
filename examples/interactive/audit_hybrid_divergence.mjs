// Locate a sampled electrical discrepancy and the surrounding scheduler work.
// This is an investigation aid, not a physical-accuracy or promotion gate.
import {readFile, writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';

const [candidatePath, referencePath, output, thresholdText='1e-7', option] = process.argv.slice(2);
assert(candidatePath && referencePath && output,
  'usage: audit_hybrid_divergence.mjs candidate.json reference.json report.json [current-threshold-A] [--allow-incomplete-motion-horizon]');
assert(option===undefined || option==='--allow-incomplete-motion-horizon');
const threshold = Number(thresholdText);
assert(Number.isFinite(threshold) && threshold > 0);
const inputs = {};
async function read(path) {
  const bytes = await readFile(path);
  inputs[path] = createHash('sha256').update(bytes).digest('hex');
  return JSON.parse(bytes);
}
const a = await read(candidatePath), b = await read(referencePath);
for (const capture of [a,b]) {
  const complete=capture.completed && capture.error===null;
  const deliberateCutoff=option==='--allow-incomplete-motion-horizon' &&
    capture.completed===false && capture.completed_steps===capture.requested_steps &&
    typeof capture.error==='string' &&
    capture.error.startsWith('simulation horizon ended before motion completed:');
  assert(complete || deliberateCutoff, 'requires complete runs or explicitly allowed motion-horizon cutoffs');
}
for (const key of ['source','world','scene_options','motor_components','policy_experiment',
  'initial_coordinates','initial_base_translation_m','step_s','control_guard_offset']) {
  assert(isDeepStrictEqual(a[key],b[key]), `incompatible ${key}`);
}
assert.equal(a.frames.length,b.frames.length,'requires matching sampled duration');
assert.equal(a.hybrid_steps.length,b.hybrid_steps.length);
const key = 'current_a';
let first = null, maximum = {difference_a:0};
for (let i=0;i<a.frames.length;i++) {
  const x=a.frames[i], y=b.frames[i];
  assert.equal(x.time_s,y.time_s,'requires identical sample times');
  assert.equal(x.motor_readings.length,y.motor_readings.length);
  for (let motor=0;motor<x.motor_readings.length;motor++) {
    const candidate=x.motor_readings[motor][key], reference=y.motor_readings[motor][key];
    assert(Number.isFinite(candidate) && Number.isFinite(reference));
    const difference_a=Math.abs(candidate-reference);
    const reading={frame:i,time_s:x.time_s,motor,dof:a.motor_components[motor].dof,
      candidate_a:candidate,reference_a:reference,difference_a};
    if (!first && difference_a>threshold) first=reading;
    if (difference_a>maximum.difference_a) maximum=reading;
  }
}
const window=first ? {start_s:a.frames[Math.max(0,first.frame-1)].time_s,end_s:first.time_s} : null;
const details=[];
let differentSegmentCounts=0;
for (let i=0;i<a.hybrid_steps.length;i++) {
  const x=a.hybrid_steps[i],y=b.hybrid_steps[i],end_s=(i+1)*a.step_s;
  if (x.accepted_segments!==y.accepted_segments) differentSegmentCounts++;
  if (window && end_s>window.start_s && end_s<=window.end_s &&
      (x.accepted_segments!==y.accepted_segments ||
       x.rejected_trials>0 || y.rejected_trials>0 ||
       x.events.some(e=>e.guard<a.control_guard_offset) ||
       y.events.some(e=>e.guard<b.control_guard_offset))) {
    details.push({nominal_step:i,end_s,candidate:x,reference:y});
  }
}
await writeFile(output,JSON.stringify({
  scope:'Same sampled duration and physical metadata. First current threshold crossing brackets a diagnostic window; temporal association does not prove causation. Legacy captures omit rejection reasons.',
  current_threshold_a:threshold,first,maximum,window,
  completion:{candidate:{completed:a.completed,error:a.error},reference:{completed:b.completed,error:b.error}},
  nominal_steps_with_different_accepted_segment_counts:differentSegmentCounts,
  rejection_diagnostics_available:a.hybrid_steps.every(s=>Array.isArray(s.rejection_details)) &&
    b.hybrid_steps.every(s=>Array.isArray(s.rejection_details)),
  window_steps:details,inputs
},null,2));
console.log(JSON.stringify({first,maximum,window,inspected_steps:details.length,output}));
