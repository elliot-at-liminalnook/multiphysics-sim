// Experiment recipes; phase redistribution and derivatives use shared Rust.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',root='runs/speed-ceiling/retiming';
const read=p=>JSON.parse(fs.readFileSync(p)),hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
const batch=process.argv[2]?read(process.argv[2]):{id:'redistributed',cases:[{blend:0,speeds:[.15]},{blend:.5,speeds:[.15,.175]},{blend:.8,speeds:[.175]}]};
assert(/^[a-z0-9-]+$/.test(batch.id)&&batch.cases.length);
assert(!fs.existsSync(`${d}/${batch.id}-trials.json`),'refusing overwrite batch');
const source=batch.source??'runs/speed-ceiling/validation/smooth-front10-v150-human-fine';
const original=read(`${source}.scene.json`),config=read(`${source}.config.json`),catalog=read(`${d}/validation-cases.json`);
const sourceSpeedChannel=original.controller.inputs.find(c=>c.name==='command.forward_speed');
assert(sourceSpeedChannel&&Number.isFinite(sourceSpeedChannel.upper)&&sourceSpeedChannel.upper>0&&sourceSpeedChannel.lower===-sourceSpeedChannel.upper);
const budgets=config.motors.effective.components.map(m=>m.parameters.no_load_speed),rows=[];
for(const definition of batch.cases) {
  const {blend,speeds}=definition;
  assert(Number.isFinite(blend)&&blend>=0&&blend<1&&speeds.length&&speeds.every(s=>Number.isFinite(s)&&s>0));
  const tag=String(blend).replace('.','p'),prefix=`${root}/${batch.id}-${tag}`;
  const recipe={trajectory:original.controller.parameters.trajectory,rate_budgets:budgets,
    redistribution:{subdivisions_per_segment:64,output_controls:160,blend,anchor_indices:[0,2,18,20,22,38,40],coordinate_intervals:definition.coordinate_intervals??null}};
  fs.writeFileSync(`${prefix}.recipe.json`,JSON.stringify(recipe)+'\n',{flag:'wx'});
  const out=fs.openSync(`${prefix}.json`,'wx'),err=fs.openSync(`${prefix}.error.log`,'wx');
  const result=spawnSync(`${bin}/redistribute_trajectory`,[`${prefix}.recipe.json`],{stdio:['ignore',out,err],timeout:30000});
  fs.closeSync(out);fs.closeSync(err);assert.equal(result.status,0);
  const report=read(`${prefix}.json`),trajectory=report.result.trajectory;
  const uniformBudget=.065/Math.max(...report.redistributed_peak_rates.map((r,i)=>r/budgets[i]));
  console.log({blend,uniform_budget_m_s:uniformBudget});
  for(const speed of speeds) {
    const name=`${batch.id}-${tag}-v${speed*1000}-human-fine`,target=`runs/speed-ceiling/validation/${name}`;
    const scene=structuredClone(original);scene.controller.parameters.trajectory=trajectory;
    const index=scene.controller.inputs.findIndex(c=>c.name==='command.forward_speed');assert(index>=0);
    scene.controller.inputs[index].lower=-speed;scene.controller.inputs[index].upper=speed;
    const actions=read(`${source}.actions.json`).map(a=>{
      assert(Number.isFinite(a[index])&&a[index]>=sourceSpeedChannel.lower-1e-15&&a[index]<=sourceSpeedChannel.upper+1e-15,'valid source command required');
      a[index]=Math.max(-speed,Math.min(speed,a[index]*speed/sourceSpeedChannel.upper));return a;
    });
    const row={name,prefix:target,source,source_scene_sha256:hash(`${source}.scene.json`),kind:'human',family:name,duration_s:20,step_s:config.step_s,command_speed_m_s:speed,
      redistribution_recipe:`${prefix}.recipe.json`,redistribution_recipe_sha256:hash(`${prefix}.recipe.json`),redistribution_report_sha256:hash(`${prefix}.json`),blend,uniform_rate_budget_m_s:uniformBudget,
      scope:'Rate-envelope phase redistribution anchored at original transfer/stance boundaries with any per-coordinate intervals explicit in the recipe, then shared periodic B-spline smoothing with 160 controls. Changes reference path; not exact old geometry. Same physical CAD/model, initialization, Rhai steering/lease/braking and 20 ms held commands. Dynamic increments absent. Zero-blend case controls for additional resampling/smoothing.'};
    for(const [suffix,value] of [['scene',scene],['config',config],['actions',actions]]) {fs.writeFileSync(`${target}.${suffix}.json`,JSON.stringify(value)+'\n',{flag:'wx'});row[`${suffix}_sha256`]=hash(`${target}.${suffix}.json`);}
    rows.push(row);catalog.rows.push(row);
  }
}
fs.writeFileSync(`${d}/${batch.id}-trials.json`,JSON.stringify({batch,rows},null,2)+'\n',{flag:'wx'});
fs.writeFileSync(`${d}/validation-cases.json`,JSON.stringify(catalog,null,2)+'\n');
