// Numerical time-grid refinement only; Rust owns all physics and optimization.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
const root='examples/full-robot/contact-implicit/';
const read=name=>JSON.parse(fs.readFileSync(root+name));
const hash=name=>createHash('sha256').update(fs.readFileSync(root+name)).digest('hex');
const source=read('resolved-contact-work.recipe.json');
const grid=read('resolved-work-halfgrid.recipe.json');
const initial=read('resolved-work-halfgrid.positions.json').positions;
assert.equal(grid.config.step_s,source.config.step_s/2);
assert.equal(initial.length,2*source.initial_positions.length-1);
const bounds=grid.config.position_reference.slice(1).map((q,k)=>q.map((v,j)=>{
  // Preserve the source's reference-relative body box and absolute joint limits.
  const coarse=Math.min(Math.floor(k/2),source.bounds.length-1);
  const offset=j<6?v-source.config.position_reference[coarse+1][j]:0;
  return {lower:source.bounds[coarse][j].lower+offset,upper:source.bounds[coarse][j].upper+offset};
}));
for(let k=0;k<bounds.length;k++)for(let j=0;j<bounds[k].length;j++)
  assert(initial[k+1][j]>=bounds[k][j].lower&&initial[k+1][j]<=bounds[k][j].upper);
const {step_s:_,position_reference:__,...newConfig}=grid.config;
const {step_s:___,position_reference:____,...oldConfig}=source.config;
assert(isDeepStrictEqual(newConfig,oldConfig),'time-grid refinement changes physical parameters/objective');
const recipe={config:grid.config,initial_positions:initial,bounds,
  search:{...source.search,maximum_iterations:100,maximum_evaluations:100000},
  smoothing_schedule_m:source.smoothing_schedule_m,
  stiffness_schedule_n_m:source.stiffness_schedule_n_m,
  hessian_scaling_exponent:source.hessian_scaling_exponent,
  provenance:{source_recipe:'resolved-contact-work.recipe.json',source_recipe_sha256:hash('resolved-contact-work.recipe.json'),
    warm_start:'resolved-work-halfgrid.positions.json',warm_start_sha256:hash('resolved-work-halfgrid.positions.json'),
    source_result_sha256:hash('resolved-contact-work.result.json'),
    operation:'Reoptimize 24 intervals at half the source planning timestep, retaining duration, objective normalization, material/contact properties, fixed initial state and reference-relative bounds. Initial guess is the previously failed linear subdivision; no prescribed contact sequence.',
    limitations:'Finite startup horizon. Planning feasibility, geometry, detailed runtime, interpolation consistency and sustained command-responsive motion require separate checks; no physical speed maximum.'}};
fs.writeFileSync(root+'resolved-work-finegrid.recipe.json',JSON.stringify(recipe,null,2)+'\n');
console.log(JSON.stringify({knots:initial.length,step_s:grid.config.step_s,free_variables:bounds.flat().length,search:recipe.search}));
