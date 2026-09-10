// Expand placement search from recorded CAD workspace samples, not chosen hip angles.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const d='examples/full-robot/contact-planning/';
const w='examples/full-robot/gait-exploration/';
const read=p=>JSON.parse(fs.readFileSync(p));
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const source=d+'joint-x25-warm.recipe.json',workspace=w+'workspace-summary.json';
const provenance=read(w+'workspace-provenance.json');
for(const [p,h] of Object.entries(provenance.sha256))assert.equal(sha(p),h,'Workspace source identity');
const oldScene=read(provenance.scene),recipe=read(source),box=read(workspace);
assert.equal(oldScene.robot.source.cad_sha256,recipe.robot.expected_cad_sha256);
const markers=read(w+'workspace-markers.json');
assert.equal(markers.expected_cad_sha256,recipe.robot.expected_cad_sha256);
const changes=[];
for(const v of recipe.variables){
  if(v.decision.kind!=='motion'||v.decision.decision.kind!=='foot_center')continue;
  const {foot,axis}=v.decision.decision;
  assert(axis<2);
  const leg=box.legs.find(l=>l.id===markers.markers[foot].id);assert(leg);
  const [lower,upper]=leg.sampled_valid_foot_bounds_world_m[axis];
  assert(Number.isFinite(lower)&&Number.isFinite(upper)&&lower<upper);
  const x=recipe.candidate.motion.feet[foot].center_world_m[axis];
  assert(x>=lower&&x<=upper,'Initial reference outside recorded workspace box');
  changes.push({foot,axis,previous:v.bound,next:{lower,upper}});
  v.bound={lower,upper};
}
assert.equal(changes.length,8);
recipe.search.maximum_outer_iterations=8;
recipe.search.maximum_evaluations=8000;
recipe.search.inner.maximum_iterations=8;
recipe.search.inner.maximum_evaluations=3000;
const output=d+'joint-workspace-speed.recipe.json';
fs.writeFileSync(output,JSON.stringify(recipe,null,2)+'\n',{flag:'wx'});
fs.writeFileSync(d+'joint-workspace-speed-preparation.json',JSON.stringify({
  inputs:[source,workspace,w+'workspace-provenance.json',w+'workspace-markers.json',provenance.scene,provenance.source]
    .map(path=>({path,sha256:sha(path)})),output:{path:output,sha256:sha(output)},changes,
  target_speed_m_s:recipe.robot.target_speed_m_s,
  scope:'Same initial motion and force seed, with XY foot-center intervals taken from the recorded CAD workspace screen. These boxes contain untested/unreachable combinations and are not certified whole-workspace bounds. Native IK, actuator, friction and collision checks remain mandatory. Hardware limits and all physical acceptance tolerances are unchanged. Larger inner evaluation budget permits several actual solver steps for 173 variables; this remains a local gait-family search, not global exhaustion.'
},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({output,placement_intervals:changes.length,evaluation_budget:recipe.search.maximum_evaluations}));
