import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import {captureOutcome} from '../../interactive/capture_outcome.mjs';
import {recordedContactMotion} from '../../interactive/recorded_contact_motion.mjs';
const root='examples/full-robot/whole-swing', read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify=s=>assert.equal(source(s.path).sha256,s.sha256,s.path);
const planPath=`${root}/loaded-foot-damping-plan.json`,plan=read(planPath);
const statusPath=`${root}/loaded-foot-damping-status.json`,status=read(statusPath);
const integrityPath=`${root}/loaded-foot-damping-integrity.json`,integrity=read(integrityPath);
assert(status.complete&&integrity.passed);
for(const r of [plan,status,integrity])r.sources.forEach(verify);
verify(plan.baseline);const baseline=read(plan.baseline.path);
const frames=r=>r.frames.map(({stepping_wall_s,...f})=>f);
const cases=status.cases.map((c,i)=>{
  const def=plan.cases[i];assert.equal(c.name,def.name);c.sources.forEach(verify);
  const capture=c.sources.find(s=>s.path.endsWith('.native.json')),r=read(capture.path);
  assert.deepEqual(r.recording.scene,baseline.recording.scene);
  const config=structuredClone(r.recording.config);
  if(def.velocity_damping_s!==null){
    assert.deepEqual(config.policy.point_feedback.floor_velocity_damping,
      {velocity_damping_s:def.velocity_damping_s,full_support_force_n:1});
    delete config.policy.point_feedback.floor_velocity_damping;
  }
  assert.deepEqual(config,baseline.recording.config);
  if(def.velocity_damping_s===null){
    assert(r.completed);assert.deepEqual(frames(r),frames(baseline));
    assert.deepEqual(r.transitions,baseline.transitions);assert.deepEqual(r.recording,baseline.recording);
    assert.deepEqual(r.contract,baseline.contract);
  }
  let checked=0,maxVelocityError=0,maxForceError=0;
  if(def.velocity_damping_s!==null)for(let j=1;j<r.frames.length;j++){
    const previous=r.frames[j-1],policy=r.frames[j].policy;
    assert(Math.abs(policy.time_s-previous.time_s)<1e-10);
    const sample=policy.point_feedback.floor_velocity_damping;assert(sample);
    config.policy.point_feedback.markers.forEach((marker,k)=>{
      const index=r.recording.scene.robot.links.findIndex(l=>l.name===marker.link);assert(index>=0);
      const motion=recordedContactMotion(previous,index,marker.link);
      maxForceError=Math.max(maxForceError,Math.abs(motion.force-sample.normal_force_n[k]));
      maxVelocityError=Math.max(maxVelocityError,...motion.velocity.map((v,axis)=>Math.abs(v-sample.contact_velocity_world_m_s[k][axis])));
      assert.equal(sample.displacement_world_m[k][2],0);checked++;
    });
  }
  assert(maxForceError<1e-10&&maxVelocityError<1e-10,'online contact observation disagrees with committed frame oracle');
  const prefix=`${root}/loaded-foot-damping-${c.name}`,outcome=captureOutcome(r);
  let metrics=null,contact=null,ratio=null;
  if(r.completed||r.frames.length>=2){
    execFileSync(process.execPath,['examples/interactive/analyze_walking_capture.mjs',capture.path,
      `${prefix}-metrics.json`,...(!r.completed?['--accepted-prefix']:[])],{stdio:'pipe'});
    metrics=read(`${prefix}-metrics.json`);
  }
  if(r.completed){
    execFileSync(process.execPath,['examples/interactive/analyze_floor_contact_motion.mjs',capture.path,`${prefix}-contact.json`],{stdio:'pipe'});
    execFileSync(process.execPath,['examples/interactive/analyze_contact_phases.mjs',capture.path,`${prefix}-phases.json`,`${prefix}-contact.json`],{stdio:'pipe'});
    contact=read(`${prefix}-contact.json`);
    if(metrics.net_horizontal_displacement_m>0)ratio=Math.max(...contact.feet.map(f=>f.integrated_load_weighted_tangential_speed_m))/metrics.net_horizontal_displacement_m;
  }
  return {name:c.name,velocity_damping_s:def.velocity_damping_s,completed:r.completed,error:c.error,outcome,
    task_passed:c.passed,acceptance:c.acceptance,
    default_frames_transitions_recording_exact:def.velocity_damping_s===null?true:null,
    contact_observation_oracle:{samples:checked,maximum_force_error_n:maxForceError,maximum_velocity_error_m_s:maxVelocityError},
    contact_motion_to_body_advance_ratio:ratio,
    minute_anti_sliding_screen_passed:c.passed&&ratio!==null&&ratio<=plan.maximum_contact_motion_to_body_advance_ratio,
    sustained_windows:metrics?.sustained_windows??[],native_compute:metrics?.native_compute??null,
    positive_mechanical_work_j:metrics?.positive_mechanical_work_j??null,capture,
    measurements:[metrics?`${prefix}-metrics.json`:null,...(contact?[`${prefix}-contact.json`,`${prefix}-phases.json`]:[])].filter(Boolean).map(source)};
});
writeFileSync(`${root}/loaded-foot-damping-summary.json`,JSON.stringify({version:1,cases,
  maximum_contact_motion_to_body_advance_ratio:plan.maximum_contact_motion_to_body_advance_ratio,
  sources:[planPath,statusPath,integrityPath,import.meta.filename,'examples/interactive/analyze_walking_capture.mjs',
    'examples/interactive/analyze_floor_contact_motion.mjs','examples/interactive/analyze_contact_phases.mjs',
    'examples/interactive/recorded_contact_motion.mjs','examples/interactive/capture_outcome.mjs'].map(source),
  browser_promoted:false,scope:plan.scope},null,2)+'\n');
console.log(JSON.stringify(cases.map(c=>({name:c.name,completed:c.completed,error:c.error,task:c.task_passed,
  ratio:c.contact_motion_to_body_advance_ratio,anti_sliding:c.minute_anti_sliding_screen_passed,
  speeds:c.sustained_windows.map(w=>w.measured_sustained_speed_m_s),oracle:c.contact_observation_oracle})),null,2));
