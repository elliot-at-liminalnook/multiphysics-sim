import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';import {captureOutcome} from '../../interactive/capture_outcome.mjs';
const root='examples/full-robot/whole-swing',read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify=s=>assert.equal(source(s.path).sha256,s.sha256,s.path);
const planPath=`${root}/feedback-ablation-open-plan.json`,plan=read(planPath);
const statusPath=`${root}/feedback-ablation-open-status.json`,status=read(statusPath);
const integrityPath=`${root}/feedback-ablation-open-integrity.json`,integrity=read(integrityPath);
assert(status.complete&&integrity.passed);for(const r of [plan,status,integrity])r.sources.forEach(verify);
plan.baseline.sources.forEach(verify);
const oldSource=plan.baseline.sources.find(s=>s.path.endsWith('.native.json')),old=read(oldSource.path);
const originalActions=read(plan.baseline_case.actions),inputs=old.recording.scene.controller.inputs;
const motion=old.recording.scene.controller.parameters.motion_command_channels.map(name=>inputs.findIndex(c=>c.name===name));
const physical=f=>{const {stepping_wall_s,...rest}=f;return rest;};
const cases=status.cases.map((c,i)=>{
  const def=plan.cases[i];assert.equal(c.name,def.name);c.sources.forEach(verify);
  const capture=c.sources.find(s=>s.path.endsWith('.native.json')),r=read(capture.path),scene=structuredClone(r.recording.scene);
  for(const change of plan.input_schema_changes){
    const input=scene.controller.inputs.find(c=>c.name===change.name);assert.equal(input[change.field],change.after);input[change.field]=change.before;
  }
  assert.deepEqual(scene,old.recording.scene);assert.deepEqual(r.recording.config,old.recording.config);
  assert.equal(r.recording.seed,old.recording.seed);assert.deepEqual(r.task,old.task);
  const changed=def.disabled_moving_gain_channels.map(name=>inputs.findIndex(c=>c.name===name));assert(changed.every(i=>i>=0));
  const actions=read(def.actions);assert.equal(actions.length,originalActions.length);
  let moving=0,stopped=0;
  actions.forEach((a,j)=>{
    const expected=[...originalActions[j]],active=motion.some(i=>expected[i]!==0);
    if(active){moving++;for(const i of changed)expected[i]=0;}else stopped++;
    assert.deepEqual(a,expected);
  });
  if(c.name==='bounds-only'){
    assert(r.completed);assert.deepEqual(r.frames.map(physical),old.frames.map(physical));
    assert.deepEqual(r.transitions,old.transitions);assert.deepEqual(r.recording.input_events,old.recording.input_events);
  }
  const outcome=captureOutcome(r),prefix=`${root}/feedback-ablation-${c.name}`;
  let metrics=null,contact=null,ratio=null,totalMotion=null;
  if(r.completed||r.frames.length>=2){
    execFileSync(process.execPath,['examples/interactive/analyze_walking_capture.mjs',capture.path,`${prefix}-metrics.json`,
      ...(!r.completed?['--accepted-prefix']:[])],{stdio:'pipe'});metrics=read(`${prefix}-metrics.json`);
  }
  if(r.completed){
    execFileSync(process.execPath,['examples/interactive/analyze_floor_contact_motion.mjs',capture.path,`${prefix}-contact.json`],{stdio:'pipe'});
    execFileSync(process.execPath,['examples/interactive/analyze_contact_phases.mjs',capture.path,`${prefix}-phases.json`,`${prefix}-contact.json`],{stdio:'pipe'});
    execFileSync(process.execPath,['examples/interactive/analyze_feedback_contributions.mjs',capture.path,`${prefix}-contributions.json`],{stdio:'pipe'});
    contact=read(`${prefix}-contact.json`);totalMotion=contact.feet.reduce((s,f)=>s+f.integrated_load_weighted_tangential_speed_m,0);
    if(metrics.net_horizontal_displacement_m>0)ratio=Math.max(...contact.feet.map(f=>f.integrated_load_weighted_tangential_speed_m))/metrics.net_horizontal_displacement_m;
  }
  return {name:c.name,completed:r.completed,error:c.error,outcome,task_passed:c.passed,acceptance:c.acceptance,
    exact_bounds_only_physical_frames:c.name==='bounds-only'?true:null,only_declared_moving_gains_changed:true,
    original_stopping_inputs_preserved:true,moving_action_samples:moving,stopped_action_samples:stopped,
    contact_motion_to_body_advance_ratio:ratio,total_integrated_foot_contact_motion_m:totalMotion,
    anti_sliding_screen_passed:c.passed&&ratio!==null&&ratio<=plan.maximum_contact_motion_to_body_advance_ratio,
    sustained_windows:metrics?.sustained_windows??[],positive_mechanical_work_j:metrics?.positive_mechanical_work_j??null,
    native_compute:metrics?.native_compute??null,capture,
    measurements:[metrics?`${prefix}-metrics.json`:null,...(contact?[`${prefix}-contact.json`,`${prefix}-phases.json`,`${prefix}-contributions.json`]:[])].filter(Boolean).map(source)};
});
writeFileSync(`${root}/feedback-ablation-summary.json`,JSON.stringify({version:1,cases,
  maximum_contact_motion_to_body_advance_ratio:plan.maximum_contact_motion_to_body_advance_ratio,
  sources:[planPath,statusPath,integrityPath,oldSource.path,import.meta.filename,'examples/interactive/analyze_walking_capture.mjs',
    'examples/interactive/analyze_floor_contact_motion.mjs','examples/interactive/analyze_contact_phases.mjs',
    'examples/interactive/analyze_feedback_contributions.mjs','examples/interactive/feedback_contributions.mjs'].map(source),
  scope:plan.scope},null,2)+'\n');
console.log(JSON.stringify(cases.map(c=>({name:c.name,completed:c.completed,error:c.error,task:c.task_passed,
  ratio:c.contact_motion_to_body_advance_ratio,total_motion_mm:c.total_integrated_foot_contact_motion_m*1000,
  speeds:c.sustained_windows.map(w=>w.measured_sustained_speed_m_s),work:c.positive_mechanical_work_j})),null,2));
