// Extend a checkpoint objective; no dynamics or alternate slip calculation.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const [controlPath,prefix,weightText]=process.argv.slice(2),weight=Number(weightText);
assert(prefix&&Number.isFinite(weight)&&weight>0,'usage: control_recipe output_prefix positive_weight');
const data=fs.readFileSync(controlPath),recipe=JSON.parse(data),w=recipe.warm_start;
assert(w&&recipe.slip_objective.constrain_loaded_slip,'actual-slip constrained checkpoint required');
assert(recipe.slip_objective.loaded_slip_barrier_weight==null,'checkpoint already has a barrier');
const groups=[...new Set(recipe.slip_objective.point_groups)].sort();
const rows=w.residuals.inequalities.slice(-groups.length);
assert(rows.every(g=>Number.isFinite(g)&&g<0),'initial sampled slip must be strictly feasible');
recipe.slip_objective.loaded_slip_barrier_weight=weight;
const residuals=rows.map(g=>Math.sqrt(weight)/-g);
w.residuals.objective.push(...residuals);
recipe.provenance={control_recipe:{path:controlPath,sha256:createHash('sha256').update(data).digest('hex')},
  operation:'Add reciprocal barrier residual sqrt(weight)/(-g) for each signed sampled actual-slip row. Preserve motion, bounds, model, constraints, multipliers, penalty schedule and solver budget. The native solver must exactly reproduce the extended checkpoint before searching. Strict feasibility covers the optimization grid only; dense audits remain separate.',
  weight,group_order:groups,initial_slip_rows:rows,initial_barrier_residuals:residuals};
fs.writeFileSync(prefix+'.recipe.json',JSON.stringify(recipe,null,2)+'\n',{flag:'wx'});
const dense=structuredClone(recipe);dense.config.periodic_collocation_phases=null;dense.config.periodic_cubic_subdivisions=16;
fs.writeFileSync(prefix+'-dense.recipe.json',JSON.stringify(dense,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({prefix,weight,rows,residuals}));
