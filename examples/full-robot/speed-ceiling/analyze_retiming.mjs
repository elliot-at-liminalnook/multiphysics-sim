// Shared Rust bounds for nonuniform traversal; no alternative trajectory math.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',root='runs/speed-ceiling/retiming';
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
fs.mkdirSync(root,{recursive:true});
const rows=[];
for(const [id,source,recipeName,oldReport] of [
  ['all8','runs/speed-ceiling/validation/smooth-hip0-v125-human-fine','smooth-capability-recipe.json','smooth-capability.json'],
  ['front10','runs/speed-ceiling/validation/smooth-front10-v150-human-fine','front10-capability-recipe.json','front10-capability.json'],
  ['front12','runs/speed-ceiling/validation/smooth-front12-v150-human-fine','front12-capability-recipe.json','front12-capability.json'],
]) {
  const baseline=read(`${d}/${oldReport}`),refinements=[];
  for(const subdivisions of [4,64,256]) {
    const prefix=`${root}/${id}-${subdivisions}`,recipe=read(`${d}/${recipeName}`);
    recipe.reference_cycle.retiming_subdivisions_per_segment=subdivisions;
    recipe.provenance={...recipe.provenance,original_recipe_sha256:hash(`${d}/${recipeName}`),source_scene_sha256:hash(`${source}.scene.json`)};
    fs.writeFileSync(`${prefix}.recipe.json`,JSON.stringify(recipe)+'\n',{flag:'wx'});
    const out=fs.openSync(`${prefix}.json`,'wx'),err=fs.openSync(`${prefix}.error.log`,'wx');
    const result=spawnSync(`${bin}/analyze_motion_capability`,[`${source}.scene.json`,'examples/full-robot/gait-exploration/workspace-markers.json',`${prefix}.recipe.json`],{stdio:['ignore',out,err],timeout:120000});
    fs.closeSync(out);fs.closeSync(err);assert.equal(result.status,0,`${id}/${subdivisions} failed`);
    const report=read(`${prefix}.json`);
    assert.equal(report.reference_cycle_rate_budget_speed_m_s,baseline.reference_cycle_rate_budget_speed_m_s,'existing uniform budget must remain exact');
    assert.deepEqual(report.reference_cycle_coordinates,baseline.reference_cycle_coordinates,'all prior rate extrema must remain exact');
    const {bounds,...retiming}=report.reference_cycle_retiming;
    if(refinements.length) {
      const old=refinements.at(-1);
      assert(bounds.duration_lower_s>=old.duration_lower_s-1e-12,'nested refinement must tighten lower bound');
      assert(bounds.duration_upper_s<=old.duration_upper_s+1e-12,'nested refinement must tighten upper bound');
    }
    refinements.push({subdivisions,cells:bounds.cells.length,duration_lower_s:bounds.duration_lower_s,duration_upper_s:bounds.duration_upper_s,
      uniform_duration_s:bounds.uniform_duration_s,...retiming,recipe_sha256:hash(`${prefix}.recipe.json`),report_sha256:hash(`${prefix}.json`)});
  }
  rows.push({id,source,source_scene_sha256:hash(`${source}.scene.json`),uniform_speed_m_s:baseline.reference_cycle_rate_budget_speed_m_s,refinements});
  const last=refinements.at(-1);console.log({id,uniform:baseline.reference_cycle_rate_budget_speed_m_s,retimed_speed:[last.speed_lower_m_s,last.speed_upper_m_s],ratio:last.uniform_to_piecewise_speed_ratio});
}
fs.writeFileSync(`${d}/retiming-summary.json`,JSON.stringify({rows,scope:'Rate-only reference-path calculation, not a controller or dynamic gait qualification. No-load budgets are provisional design budgets, not hard motor/backdrive limits. Acceleration, torque, phase continuity, support/contact and body stability remain unresolved. Nested bounds use f64 arithmetic; exact existing uniform-rate reports are checked.'},null,2)+'\n',{flag:'wx'});
