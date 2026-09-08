import {readFileSync,writeFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing', read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const checker='examples/interactive/check_profile_capture.mjs';
const repeated=['runs/full-robot/learning/whole-combined/combined-minute-1.25ms.native.json','runs/full-robot/learning/whole-minute-refinement/combined-minute-1.25ms.native.json'];
const exactRepeat=JSON.parse(execFileSync(process.execPath,[checker,...repeated],{encoding:'utf8'}));
const profiled=['runs/full-robot/learning/whole-small-secants/small-secants-enabled.native.json','runs/full-robot/learning/whole-small-secants/enabled.profiled.native.json'];
const exactProfile=JSON.parse(execFileSync(process.execPath,[checker,...profiled],{encoding:'utf8'}));
const profilePath='runs/full-robot/learning/whole-small-secants/enabled.profile.json', profile=read(profilePath);
const physical=read(`${root}/minute-refinement-status.json`), difference=read(`${root}/minute-fine-difference.json`);
assert(physical.complete && physical.cases.every(c=>c.passed));
const budgets={maximum_foot_difference_m:.001,maximum_body_difference_m:.0005};
const numerical=difference.metrics.foot_marker_position_m.maximum<=budgets.maximum_foot_difference_m
  && difference.metrics.body_position_m.maximum<=budgets.maximum_body_difference_m;
assert(numerical);
writeFileSync(`${root}/fine-reference-status.json`,JSON.stringify({version:1,exact_repeat:exactRepeat,exact_profile:exactProfile,
  minute_numerical_passed:numerical,budgets,maximum_foot_difference_m:difference.metrics.foot_marker_position_m.maximum,
  maximum_body_difference_m:difference.metrics.body_position_m.maximum,
  small_secants_profile:{completed:profile.completed,wall_s:profile.wall_s,buckets:profile.buckets},
  sources:[...repeated,...profiled,profilePath,checker,`${root}/minute-refinement-status.json`,`${root}/minute-fine-difference.json`,
    `${root}/small-secants-status.json`,`${root}/small-secants-difference.json`,`${root}/collect_fine_reference.mjs`].map(source),
  scope:'Full minute physical acceptance and declared sampled timestep screen at 1.25/0.625 ms; repeated 1.25 ms physical/task/replay frames exactly preserved. Small secants preserve their physical case but do not reduce derivative count or measured time. No browser realtime, held-out robustness or hardware calibration claim.'},null,2)+'\n');
console.log({minute_numerical_passed:numerical,small_secants_fresh_jacobians:profile.buckets.find(b=>b.name==='fresh jacobians').calls});
