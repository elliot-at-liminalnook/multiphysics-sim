import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const planPath=`${root}/tangent-radius-plan.json`,statusPath=`${root}/tangent-radius-status.json`,browserPath=`${root}/tangent-radius-browser-parity.json`;
const plan=read(planPath),status=read(statusPath),browser=read(browserPath);assert(status.complete&&browser.complete);
const candidates=plan.cases.map(c=>{
  const native=status.cases.find(r=>r.name===c.name),host=browser.cases.find(r=>r.name===c.name);
  const comparisonPath=`${root}/${c.name}-difference.json`,difference=read(comparisonPath);
  const passed=native.passed&&host.measurement?.passed&&host.measurement.replay_exact&&host.measurement.reset_exact
    &&difference.metrics.foot_marker_position_m.maximum<=.001&&difference.metrics.body_position_m.maximum<=.0005;
  return {name:c.name,probe_relative_step:c.probe_relative_step,passed:Boolean(passed),worker_p95_s:host.measurement?.performance.transition_p95_s,
    physical_passed:native.passed,host_passed:host.measurement?.passed??false,comparison:source(comparisonPath)};
});
const selected=candidates.filter(c=>c.passed).sort((a,b)=>a.worker_p95_s-b.worker_p95_s)[0];assert(selected,'No radius passed all selection screens');
const recipe=plan.cases.find(c=>c.name===selected.name),native=status.cases.find(c=>c.name===selected.name);
for(const key of ['scene','config']){
  assert.equal(source(recipe[key]).sha256,native.sources.find(s=>s.path===recipe[key]).sha256);
  writeFileSync(`${root}/portable-tangent-turn.${key}.json`,readFileSync(recipe[key]));
}
const report={version:1,selected,candidates,sources:[planPath,statusPath,browserPath,import.meta.filename].map(source),
  scope:'Development selection: lowest measured sequential worker-only p95 among radii passing unchanged native task, solver difference, host portability and exact replay/reset screens. One timing sample per radius; no statistical performance ranking. Rendered realtime, sustained walking, timestep accuracy and held-out robustness require separate evidence.'};
writeFileSync(`${root}/tangent-radius-selection.json`,JSON.stringify(report,null,2)+'\n');console.log(selected);
