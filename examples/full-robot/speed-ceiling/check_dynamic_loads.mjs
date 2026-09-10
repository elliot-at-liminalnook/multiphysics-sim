import fs from 'node:fs';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling', root=process.argv[2]??'runs/speed-ceiling/dynamic-checks';
const read=p=>JSON.parse(fs.readFileSync(p));
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
fs.mkdirSync(root,{recursive:true});
const scenePath='runs/speed-ceiling/validation/smooth-hip0-v125-human-fine.scene.json';
const markerPath='examples/full-robot/gait-exploration/workspace-markers.json';
const recipe=read('runs/speed-ceiling/validation/dynamic-v125.forward.load-recipe.json');
recipe.reference.sample_times_s=recipe.reference.sample_times_s.slice(0,5);
recipe.support_weights=recipe.support_weights.slice(0,5);
recipe.reference.phase_rate=0;recipe.reference.phase_acceleration_per_s=0;
recipe.reference.base_velocity.fill(0);recipe.reference.base_acceleration.fill(0);
const results=[];
function run(name, value, success, scene=scenePath, markers=markerPath) {
  const path=`${root}/${name}.recipe.json`;
  fs.writeFileSync(path,JSON.stringify(value)+'\n',{flag:'wx'});
  const result=spawnSync(`${bin}/reference_load_feedforward`,[scene,markers,path],{encoding:'utf8',maxBuffer:32*1024*1024,timeout:60000});
  fs.writeFileSync(`${root}/${name}.output.json`,result.stdout??'',{flag:'wx'});
  fs.writeFileSync(`${root}/${name}.error.txt`,result.stderr??'',{flag:'wx'});
  assert.equal(result.status===0,success,`${name}: ${result.stderr}`);
  results.push({name,expected_success:success,exit:result.status,error:result.stderr.trim()});
  return success?JSON.parse(result.stdout):null;
}
const zero=run('zero-dynamic',recipe,true);
assert(zero.dynamic_increment_offsets_rad.flat().every(v=>Math.abs(v)<1e-12));
const legacy=structuredClone(recipe);delete legacy.reference;legacy.samples=zero.frames.map(f=>f.coordinates);
const stat=run('legacy-static',legacy,true);
let maximum=0;for(let i=0;i<zero.target_offsets_rad.length;i++)for(let j=0;j<zero.target_offsets_rad[i].length;j++)maximum=Math.max(maximum,Math.abs(zero.target_offsets_rad[i][j]-stat.target_offsets_rad[i][j]));
assert(maximum<1e-12,'zero dynamic must reproduce static allocation');
const huge=structuredClone(recipe);huge.support_weights[0]=[1e308,1e308,1e308,1e308];run('overflow-support-sum',huge,false);
const linear=structuredClone(recipe);linear.reference.trajectory.interpolation='linear';run('discontinuous-reference',linear,false);
const mixed=structuredClone(recipe);mixed.samples=legacy.samples;run('ambiguous-reference',mixed,false);
const scene=read(scenePath), markers=read(markerPath);delete scene.robot.source.cad_sha256;markers.expected_cad_sha256=null;
const missingScene=`${root}/missing-hash.scene.json`,missingMarkers=`${root}/missing-hash.markers.json`;
fs.writeFileSync(missingScene,JSON.stringify(scene)+'\n',{flag:'wx'});fs.writeFileSync(missingMarkers,JSON.stringify(markers)+'\n',{flag:'wx'});
run('both-cad-hashes-missing',recipe,false,missingScene,missingMarkers);
const archivedPrefix='runs/speed-ceiling/clearance/return28-lift12-v125-load1';
const archived=read(`${archivedPrefix}.load-table.json`);
const reproduced=run('archived-static',read(`${archivedPrefix}.load-recipe.json`),true,'runs/speed-ceiling/clearance/return28-lift12-v125.scene.json');
assert.deepEqual(reproduced.target_offsets_rad,archived.target_offsets_rad,'archived static target offsets changed');
const report={passed:true,maximum_zero_dynamic_static_difference_rad:maximum,results,
  archived_static_targets_exact:true,
  example_source_sha256:crypto.createHash('sha256').update(fs.readFileSync('crates/sim-runtime/examples/reference_load_feedforward.rs')).digest('hex'),
  scope:'CLI behavior and zero-motion compatibility checks. Analytic inertia/gravity validation remains in sim-domain-robot inverse_loads; these checks do not validate gait contact assumptions.'};
fs.writeFileSync(`${d}/dynamic-load-checks.json`,JSON.stringify(report,null,2)+'\n');console.log(report);
