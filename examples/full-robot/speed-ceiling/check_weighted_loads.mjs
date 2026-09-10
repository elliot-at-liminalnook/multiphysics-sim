import fs from 'node:fs';import assert from 'node:assert/strict';import {spawnSync} from 'node:child_process';import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',root=process.argv[2]??'runs/speed-ceiling/weighted-checks',reportName=process.argv[3]??'weighted-load-checks';
assert(/^[a-z0-9-]+$/.test(reportName));fs.mkdirSync(root,{recursive:true});
const read=p=>JSON.parse(fs.readFileSync(p)),hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
const source=read(`${d}/front-dynamic-trials.json`).rows.find(r=>r.name==='smooth-dynamic-front10-v150-scale1-human-fine');assert(source);
const recipe=read(source.tables.forward.recipe),scene=`${source.source}.scene.json`,markers='examples/full-robot/gait-exploration/workspace-markers.json',results=[];
function run(name,value,success) {
  const path=`${root}/${name}.recipe.json`;fs.writeFileSync(path,JSON.stringify(value)+'\n',{flag:'wx'});
  const r=spawnSync(`${bin}/reference_load_feedforward`,[scene,markers,path],{encoding:'utf8',maxBuffer:32*1024*1024,timeout:120000});
  fs.writeFileSync(`${root}/${name}.json`,r.stdout??'',{flag:'wx'});fs.writeFileSync(`${root}/${name}.error.log`,r.stderr??'',{flag:'wx'});
  assert.equal(r.status===0,success,`${name}: ${r.stderr}`);results.push({name,exit:r.status,error:r.stderr.trim()});return success?JSON.parse(r.stdout):null;
}
const old=read(source.tables.forward.table),same=run('default-compatibility',recipe,true);
assert.deepEqual(same.target_offsets_rad,old.target_offsets_rad);assert.deepEqual(same.dynamic_increment_offsets_rad,old.dynamic_increment_offsets_rad);
for(let i=0;i<same.frames.length;i++)for(const key of Object.keys(old.frames[i]))assert.deepEqual(same.frames[i][key],old.frames[i][key]);
const zero=structuredClone(recipe);zero.reference.sample_times_s=zero.reference.sample_times_s.slice(0,5);zero.support_weights=zero.support_weights.slice(0,5);
zero.reference.phase_rate=0;zero.reference.phase_acceleration_per_s=0;zero.reference.base_velocity.fill(0);zero.reference.base_acceleration.fill(0);
zero.wrench_allocation={length_scale_m:.35,friction_coefficient:read(`${d}/front10-capability-recipe.json`).support_friction_coefficient};
const result=run('weighted-zero-motion',zero,true);assert(result.dynamic_increment_offsets_rad.flat().every(v=>Math.abs(v)<1e-12));
const invalid=structuredClone(zero);invalid.wrench_allocation.length_scale_m=0;run('zero-wrench-scale',invalid,false);
invalid.wrench_allocation.length_scale_m=.35;invalid.wrench_allocation.friction_coefficient=-.1;run('negative-friction',invalid,false);
const disabled=structuredClone(zero);disabled.support_weights[0].fill(0);run('no-active-support',disabled,false);
zero.wrench_allocation.constrained={regularization:.0001,maximum_iterations:20000,gradient_tolerance_n:.000001};
const constrained=run('constrained-zero-motion',zero,true);
assert(constrained.dynamic_increment_offsets_rad.flat().every(v=>Math.abs(v)<1e-12));
assert(constrained.frames.every(f=>f.wrench_allocation.optimizer.converged&&f.wrench_allocation.unilateral_friction_satisfied));
const short=structuredClone(zero);short.wrench_allocation.constrained.maximum_iterations=1;
const unfinished=run('constrained-iteration-limit',short,true);
assert(unfinished.frames.some(f=>!f.wrench_allocation.optimizer.converged),'iteration limit must not imply convergence');
assert(unfinished.frames.every(f=>f.wrench_allocation.unilateral_friction_satisfied));
const bad=structuredClone(zero);bad.wrench_allocation.constrained.regularization=0;run('invalid-regularization',bad,false);
fs.writeFileSync(`${d}/${reportName}.json`,JSON.stringify({passed:true,default_offsets_and_existing_frame_fields_exact:true,zero_motion_increment_zero:true,source_table_sha256:hash(source.tables.forward.table),results,
  scope:'Default dynamic table and all existing frame fields remain exact. Weighted zero-motion increments vanish and malformed allocation inputs reject. Analytic point-wrench tests are in the shared robot library; no gait contact or support feasibility claim.'},null,2)+'\n',{flag:'wx'});
