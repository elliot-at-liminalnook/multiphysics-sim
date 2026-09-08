import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',run='runs/full-robot/learning/whole-velocity-seed',read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const p95=a=>[...a].sort((a,b)=>a-b)[Math.ceil(.95*a.length)-1];
const cases=['reference','enabled'].map(kind=>{
  const path=`${run}/velocity-seed-${kind}.native.json`,r=read(path);assert(r.completed&&!r.error);
  const samples=r.frames.slice(1).map((f,i)=>({phase:f.policy.step_reference.reference.phase,wall_s:f.stepping_wall_s-r.frames[i].stepping_wall_s}));
  assert(samples.every(s=>Number.isFinite(s.wall_s)&&s.wall_s>=0));
  return {name:`velocity-seed-${kind}`,wall_s:r.wall_s,transitions:samples.length,p95_s:p95(samples.map(s=>s.wall_s)),
    active_p95_s:p95(samples.filter(s=>!['hold','idle'].includes(s.phase)).map(s=>s.wall_s)),
    phase_p95_s:Object.fromEntries([...new Set(samples.map(s=>s.phase))].map(p=>[p,p95(samples.filter(s=>s.phase===p).map(s=>s.wall_s))])),source:source(path)};
});
const differencePath=`${root}/velocity-seed-difference.json`,difference=read(differencePath);
assert(difference.metrics.foot_marker_position_m.maximum<=.001&&difference.metrics.body_position_m.maximum<=.0005);
const report={version:1,cases,solver_difference_passed:true,browser_promoted:false,
  sources:[differencePath,`${root}/velocity-seed-status.json`,`${root}/velocity-seed-profile.json`,`${root}/velocity-seed-integrity.json`,import.meta.filename].map(source),
  scope:'Native cumulative stepping clocks from the isolated unprofiled captures; excludes browser transfer/drawing and native final serialization. Predictor slightly worsens total time and active/return p95 despite fewer Newton iterations/Jacobians, because proposal screening adds mapping work. Retained default-off; no new WASM performance claim. Physical, exact default preservation and profile results are separate linked evidence.'};
writeFileSync(`${root}/velocity-seed-summary.json`,JSON.stringify(report,null,2)+'\n');console.log(cases.map(c=>({name:c.name,active_p95_s:c.active_p95_s})));
