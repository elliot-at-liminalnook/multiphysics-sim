// Derive a height-search interval from recorded CAD reach samples. Physics and
// all subsequent trajectory evaluation remain in the existing Rust runtime.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const d='examples/full-robot/contact-planning/',w='examples/full-robot/gait-exploration/';
const read=p=>JSON.parse(fs.readFileSync(p));
const identity=path=>({path,bytes:fs.statSync(path).size,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const source=d+'joint-aligned-speed.recipe.json',scene='runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json';
const original=read(source),recipe=structuredClone(original),workspace=read(w+'workspace-summary.json'),provenance=read(w+'workspace-provenance.json'),markers=read(w+'workspace-markers.json');
for(const [path,hash] of Object.entries(provenance.sha256))assert.equal(identity(path).sha256,hash,'Workspace provenance mismatch');
const sampledModel=read(provenance.scene).robot,currentModel=read(scene).robot;
assert.equal(sampledModel.source.cad_sha256,recipe.robot.expected_cad_sha256);
assert.equal(currentModel.source.cad_sha256,recipe.robot.expected_cad_sha256);
assert.equal(markers.expected_cad_sha256,recipe.robot.expected_cad_sha256);
assert.equal(markers.coordinate_frame,'r1357-export-world-Z-up-m');
// Validation has refined collision fields. The CAD link frames, mass data and
// joints underlying the recorded workspace must otherwise agree exactly.
const stripCollision=link=>{const copy=structuredClone(link);delete copy.collision;return copy;};
assert.equal(sampledModel.links.length,currentModel.links.length);
for(const link of sampledModel.links){const current=currentModel.links.find(l=>l.id===link.id);assert(current);assert(isDeepStrictEqual(stripCollision(link),stripCollision(current)),'Changed CAD link definition');}
assert(isDeepStrictEqual(sampledModel.joints,currentModel.joints),'Changed CAD joint definitions');
assert.equal(markers.markers.length,recipe.candidate.motion.feet.length);
const rows=markers.markers.map((marker,foot)=>{
  const leg=workspace.legs.find(l=>l.id===marker.id);assert(leg);
  const [minimum,maximum]=leg.sampled_valid_foot_bounds_world_m[2];
  assert(Number.isFinite(minimum)&&Number.isFinite(maximum)&&minimum<maximum);
  const target=recipe.candidate.motion.feet[foot].center_world_m[2],initial=recipe.robot.initial_base_translation_m[2];
  return {foot,marker:marker.id,sampled_foot_z_m:[minimum,maximum],target_foot_z_m:target,initial_base_translation_z_m:initial,body_control_z_interval_m:[target-maximum-initial,target-minimum-initial]};
});
const interval={lower:Math.max(...rows.map(r=>r.body_control_z_interval_m[0])),upper:Math.min(...rows.map(r=>r.body_control_z_interval_m[1]))};
assert(interval.lower<interval.upper,'Empty sampled height interval');
const changes=[];
for(const v of recipe.variables){
  if(v.decision.kind!=='motion'||v.decision.decision.kind!=='body_control'||v.decision.decision.channel!==2)continue;
  assert(interval.lower<=v.bound.lower&&interval.upper>=v.bound.upper,'Derived interval would shrink an existing search bound');
  const control=v.decision.decision.control,value=recipe.candidate.motion.body.keyframes[control].values[2];
  assert(value>=interval.lower&&value<=interval.upper);
  changes.push({control,previous:structuredClone(v.bound),next:structuredClone(interval)});v.bound=structuredClone(interval);
}
assert.equal(changes.length,8);
const restored=structuredClone(recipe);
for(const v of restored.variables){if(v.decision.kind==='motion'&&v.decision.decision.kind==='body_control'&&v.decision.decision.channel===2)v.bound=changes.find(c=>c.control===v.decision.decision.control).previous;}
assert(isDeepStrictEqual(restored,original),'Changed more than height search bounds');
const output=d+'joint-height-speed.recipe.json';fs.writeFileSync(output,JSON.stringify(recipe,null,2)+'\n',{flag:'wx'});
const record={inputs:[source,scene,w+'workspace-summary.json',w+'workspace-provenance.json',w+'workspace-markers.json',provenance.scene,provenance.source].map(identity),output:identity(output),rows,intersection:interval,changes,checks:{cad_hashes_match:true,non_collision_link_definitions_identical:true,joint_definitions_identical:true,only_height_search_bounds_changed:true,initial_candidate_identical:true,physical_gates_identical:true},formula:'For each foot, floor target = sampled foot Z + initial base translation Z + body control Z. Intersect [target - sampled maximum - initial, target - sampled minimum - initial] across all feet.',scope:'Experimental planning domain derived from fixed-base sampled CAD reach, not certified hardware limits or the entire reachable workspace. The original workspace omitted its initial base vertical offset; that offset is accounted for explicitly here. The interval contains untested combinations and is conditional on sampled body orientation; swing motion, changing orientation, collision and actuator feasibility still require the unchanged native checks. No robot/world/actuator property or initial motion/force curve is changed.'};
fs.writeFileSync(d+'joint-height-workspace-preparation.json',JSON.stringify(record,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({interval,changed_controls:changes.length,variables:recipe.variables.length,checks:record.checks}));
