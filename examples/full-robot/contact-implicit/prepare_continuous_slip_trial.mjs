// Recipe composition only; all slip and physical evaluations use shared Rust.
import fs from 'node:fs';import assert from 'node:assert/strict';import {createHash} from 'node:crypto';
const [controlPath,prefix,weightText]=process.argv.slice(2),weight=Number(weightText);
assert(prefix&&Number.isFinite(weight)&&weight>0,'usage: control_recipe output_prefix positive_weight');
const data=fs.readFileSync(controlPath),recipe=JSON.parse(data);
assert(!recipe.warm_start,'cold recorded seed required; do not carry mismatched objective checkpoints');
assert(!recipe.slip_objective.constrain_loaded_slip&&!recipe.slip_objective.loaded_slip_barrier_weight&&!recipe.slip_objective.use_continuous_mean_bound,'unmodified RMS control required');
recipe.slip_objective.use_continuous_mean_bound=true;
recipe.slip_objective.loaded_slip_barrier_weight=weight;
recipe.provenance={control_recipe:{path:controlPath,sha256:createHash('sha256').update(data).digest('hex')},
  operation:'Change slip shaping to the continuous mean upper bound and add its strict reciprocal barrier. Preserve initial recorded motion, physical configuration, bounds, displacement, grid and solver settings. Native initial evaluation must be strictly interior. No hard-cutoff slip rows are added; the sufficient mean bound protects actual sampled slip.',weight};
fs.writeFileSync(prefix+'.recipe.json',JSON.stringify(recipe,null,2)+'\n',{flag:'wx'});
const dense=structuredClone(recipe);dense.config.periodic_collocation_phases=null;dense.config.periodic_cubic_subdivisions=16;
fs.writeFileSync(prefix+'-dense.recipe.json',JSON.stringify(dense,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({prefix,weight,search:recipe.search}));
