// Additive angular-target diagnostic, not force cancellation or a new controller.
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {correctionInteraction} from './feedback_contributions.mjs';
const [capturePath,output]=process.argv.slice(2);assert(capturePath&&output);
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const r=JSON.parse(readFileSync(capturePath));assert(r.completed&&!r.error);
const parameters=r.recording.scene.controller.parameters,phases={};let maxFormulaError=0;
for(const f of r.frames.slice(1)){
  const p=f.policy,s=p.observations;
  if(parameters.motion_command_channels.every(k=>s[k]===0))continue;
  const body=[],point=[];
  for(const [target,actual] of Object.entries(p.targets)){
    const j=target.slice(0,-7),reference=s[j+'.reference'];
    body.push(s['command.body_gain']*s[j+'.body_correction']);
    point.push(s['command.point_gain']*s[j+'.point_correction']);
    const expected=reference+s['command.tracking_gain']*(reference-s[j+'.angle'])+body.at(-1)+point.at(-1)
      +s[parameters.residual_input_by_target[target]];
    assert(Number.isFinite(expected));maxFormulaError=Math.max(maxFormulaError,Math.abs(actual-expected));
  }
  const v=correctionInteraction(body,point),phase=p.step_reference.reference.phase;
  const a=phases[phase]??={samples:0,dot:0,body_squared:0,point_squared:0,cancelled:0,weight:0,opposed:0,body_error:0};
  a.samples++;a.dot+=v.dot;a.body_squared+=v.bodyNorm**2;a.point_squared+=v.pointNorm**2;
  a.cancelled+=v.cancelledNorm;a.weight+=v.bodyNorm+v.pointNorm;a.opposed+=Number(v.opposed);
  a.body_error+=Math.hypot(...p.body_feedback.position_error_world_m);
}
assert(maxFormulaError<1e-10,'recorded targets do not follow the declared moving additive-feedback decomposition');
const result={version:1,maximum_target_reconstruction_error_rad:maxFormulaError,
  phases:Object.fromEntries(Object.entries(phases).map(([phase,a])=>[phase,{samples:a.samples,
    aggregate_cosine:a.body_squared*a.point_squared>0?a.dot/Math.sqrt(a.body_squared*a.point_squared):null,
    cancellation_fraction:a.weight>0?a.cancelled/a.weight:null,opposed_sample_fraction:a.opposed/a.samples,
    body_correction_rms_rad:Math.sqrt(a.body_squared/a.samples),point_correction_rms_rad:Math.sqrt(a.point_squared/a.samples),
    mean_body_error_m:a.body_error/a.samples}])),
  sources:[capturePath,import.meta.filename,'examples/interactive/feedback_contributions.mjs'].map(source),
  scope:'Only nonzero motion-command samples whose actual targets reconstruct from reference, tracking, body, point and residual contributions. Angular suggestion cancellation is not physical force cancellation or causal proof; stopped integral behavior is excluded.'};
writeFileSync(output,JSON.stringify(result,null,2)+'\n');console.log(result.phases);
