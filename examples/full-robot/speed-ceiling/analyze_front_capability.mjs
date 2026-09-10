import fs from 'node:fs';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
for(const lift of [10,12]) {
  const source=`runs/speed-ceiling/validation/smooth-front${lift}-v150-human-fine`,scene=read(`${source}.scene.json`),config=read(`${source}.config.json`);
  const recipe=read(`${d}/smooth-capability-recipe.json`);
  recipe.reference_cycle.samples=scene.controller.parameters.trajectory.keyframes.map(k=>k.values);
  recipe.inspection.samples=[{id:`front${lift}-initial`,coordinates:config.initial_coordinates}];
  recipe.provenance={source,source_scene_sha256:hash(`${source}.scene.json`),scope:'Only +X nominal lift height changed. Exact extrema of shared periodic B-spline reference; conditional no-load budget, not loaded gait or global speed limit.'};
  const prefix=`${d}/front${lift}-capability`;
  fs.writeFileSync(`${prefix}-recipe.json`,JSON.stringify(recipe,null,2)+'\n',{flag:'wx'});
  const out=fs.openSync(`${prefix}.json`,'wx'),err=fs.openSync(`${prefix}-error.log`,'wx');
  const result=spawnSync(`${bin}/analyze_motion_capability`,[`${source}.scene.json`,'examples/full-robot/gait-exploration/workspace-markers.json',`${prefix}-recipe.json`],{stdio:['ignore',out,err],timeout:120000});fs.closeSync(out);fs.closeSync(err);
  if(result.status!==0)throw Error(`capability analysis failed ${lift}`);
  console.log({lift,budget_m_s:read(`${prefix}.json`).reference_cycle_rate_budget_speed_m_s});
}
