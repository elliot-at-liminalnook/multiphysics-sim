import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',run='runs/full-robot/learning/fast-student-fidelity';
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const statusPath=`${root}/fast-student-fidelity-status.json`,status=read(statusPath);assert(status.complete);
const auditPath=`${root}/fast-student-fidelity-integrity.json`;assert(read(auditPath).passed);
assert(read(`${root}/fast-student-unused-feedback.json`).passed);
const cases=status.cases.map(c=>{
  const path=`${run}/${c.name}.metrics.json`,capture=c.sources.find(s=>s.path.endsWith('.native.json'));
  execFileSync(process.execPath,['examples/interactive/analyze_walking_capture.mjs',capture.path,path,...(!c.completed?['--accepted-prefix']:[])],{stdio:'ignore'});
  const metrics=read(path);assert.equal(metrics.capture.sha256,capture.sha256);return {...c,metrics,metrics_source:source(path)};
});
const comparisons=['fast-student-coarse-refinement.json','fast-student-fine-refinement.json'].map(name=>{
  const path=`${root}/${name}`,r=read(path),foot=r.metrics.foot_marker_position_m.maximum,body=r.metrics.body_position_m.maximum;
  return {source:source(path),foot_difference_m:foot,body_difference_m:body,passed:foot<=.001&&body<=.0005};
});
const sources=[statusPath,auditPath,`${root}/FAST-STUDENT-FIDELITY-PLAN.md`,`${root}/fast-student-unused-feedback.json`,import.meta.filename];
writeFileSync(`${root}/fast-student-fidelity-summary.json`,JSON.stringify({version:1,cases,comparisons,sources:sources.map(source),
  scope:'Fixed network and physical model. Timestep sensitivity and endpoint acceptance retain their original budgets; no favorable checkpoint selected using these outcomes. Exact omission of unused motor-feedback calculations leaves planner observations and physical contacts intact. Browser host parity/performance and held-out terrain remain separate.'},null,2)+'\n');
console.log({comparisons,cases:cases.map(c=>({name:c.name,passed:c.passed,error:c.error}))});
