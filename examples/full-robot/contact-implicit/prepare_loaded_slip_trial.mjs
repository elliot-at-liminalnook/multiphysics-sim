// Add actual-slip inequalities to an existing continuation; no physics here.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
import {createHash} from 'node:crypto';
const [controlPath,parentPath,prefix]=process.argv.slice(2);
assert(prefix,'usage: control_continuation_recipe parent_result output_prefix');
const read=p=>JSON.parse(fs.readFileSync(p));
const identity=path=>({path,sha256:createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const recipe=read(controlPath),parent=read(parentPath),w=recipe.warm_start;
assert(w&&!recipe.slip_objective.constrain_loaded_slip,'unlimited continuation required');
assert(same(recipe.initial_positions,parent.positions),'parent initial motion mismatch');
assert(same(w.values,parent.result.search.values)&&same(w.residuals,parent.result.search.residuals)&&same(w.multipliers,parent.result.search.multipliers),'parent checkpoint mismatch');
assert(same(recipe.slip_objective,parent.slip_objective),'parent slip definition mismatch');
const rows=parent.slip.groups.map(g=>(g.sampled_loaded_slip_ratio-recipe.slip_objective.target_ratio)/recipe.slip_objective.ratio_scale);
assert(rows.length>0&&rows.every(v=>Number.isFinite(v)&&v<=0),'this adapter requires an initially passing sampled slip state');
recipe.slip_objective.constrain_loaded_slip=true;
w.residuals.inequalities.push(...rows);w.multipliers.push(...rows.map(()=>0));
recipe.provenance={control_recipe:identity(controlPath),parent_result:identity(parentPath),
  operation:'Matched continuation adds actual loaded-slip inequalities after all physical rows. Preserve every previous multiplier and initialize only the added rows to zero. Initial added rows are nonpositive, so the prior shifted norm and next penalty are unchanged. The native solver must exactly re-evaluate this extended checkpoint before any search step.',
  group_order:parent.slip.groups.map(g=>g.group),initial_added_rows:rows,
  unchanged:'Motion, displacement, contact/actuator model, bounds, 128-time grid, RMS objective, scaling, budgets and external gates match the recorded control.'};
fs.writeFileSync(prefix+'.recipe.json',JSON.stringify(recipe,null,2)+'\n',{flag:'wx'});
const dense=structuredClone(recipe);dense.config.periodic_collocation_phases=null;dense.config.periodic_cubic_subdivisions=16;
fs.writeFileSync(prefix+'-dense.recipe.json',JSON.stringify(dense,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({prefix,added_groups:rows.length,initial_rows:rows}));
