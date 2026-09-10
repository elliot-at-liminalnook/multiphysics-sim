// Orchestration/provenance only. The Rust sampler converts recorded CAD motion.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
const [binary,scenePath,capturePath,sourcePath,startText,outPrefix,displacementMode,...extra]=process.argv.slice(2);
assert(outPrefix&&!extra.length&&(!displacementMode||displacementMode==='--fix-recorded-displacement'),
  'usage: node prepare_recorded_periodic_trial.mjs sampler scene capture source_recipe start_s output_prefix [--fix-recorded-displacement]');
const read=p=>JSON.parse(fs.readFileSync(p));
const recipe=read(sourcePath),scene=read(scenePath),capture=read(capturePath);
assert.equal(scene.robot.source.cad_sha256,recipe.config.expected_cad_sha256);
assert.equal(capture.recording.scene.robot.source.cad_sha256,recipe.config.expected_cad_sha256);
assert.equal(capture.recording.scene.robot.world.floor_z,scene.robot.world.floor_z);
capture.metadata.joint_indices.forEach((index,j)=>{
  const coordinate=capture.metadata.frame_coordinates.find(c=>c.index===index);
  assert.equal(coordinate.name,recipe.config.independent_coordinates[j]);
  assert.equal(coordinate.position_unit,'rad');
});
const seed=JSON.parse(execFileSync(binary,[scenePath,capturePath,sourcePath,startText],{encoding:'utf8'}));
const encoded=seed.positions.slice(0,-1).concat([[seed.positions.at(-1)[0]-seed.positions[0][0],seed.positions.at(-1)[1]-seed.positions[0][1]]]);
if(displacementMode) recipe.bounds[recipe.bounds.length-1]=encoded.at(-1).map(v=>({lower:v,upper:v}));
encoded.forEach((q,k)=>q.forEach((v,j)=>assert(v>=recipe.bounds[k][j].lower&&v<=recipe.bounds[k][j].upper,'strict seed bound')));
const hash=path=>({path,sha256:createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
recipe.initial_positions=seed.positions;
recipe.provenance={...recipe.provenance,initial_guess:'Recorded runtime motion; numerical seed only',seed:null,
  capture:hash(capturePath),sampler:hash('crates/sim-runtime/examples/sample_capture_seed.rs'),sampler_binary:hash(binary),
  source_recipe:sourcePath,source_recipe_sha256:hash(sourcePath).sha256,
  transformation:seed.scope,start_s:seed.start_s,removed_endpoint_drift:seed.removed_endpoint_drift,
  displacement_bound:displacementMode?'Explicitly fix XY displacement to the sampled runtime cycle; not a speed maximum or a change to robot limits.':'Preserve supplied displacement bounds.',
  source_commands:capture.frames.filter(f=>f.time_s>=seed.start_s&&f.time_s<=seed.start_s+seed.period_s)
    .map(f=>({time_s:f.time_s,values:Object.fromEntries(Object.entries(f.policy.observations).filter(([k])=>k.includes('command')))}))};
for(const [suffix,data] of [['initial',seed],['recipe',recipe]])
  fs.writeFileSync(outPrefix+'.'+suffix+'.json',JSON.stringify(data,null,2)+'\n',{flag:'wx'});
