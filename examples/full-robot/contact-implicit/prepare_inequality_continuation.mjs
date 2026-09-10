// Resume an outer-iteration boundary without dropping its dual information.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
import {createHash} from 'node:crypto';
const [recipePath,resultPath,initialAuditPath,prefix,outerText]=process.argv.slice(2);
assert(prefix&&outerText,'usage: source_recipe source_result initial_audit_or_dash output_prefix additional_outer_iterations');
const read=p=>JSON.parse(fs.readFileSync(p));
const identity=path=>({path,sha256:createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const recipe=read(recipePath),result=read(resultPath),search=result.result.search;
const canonicalConfig=c=>{const copy=structuredClone(c);if(copy.periodic_collocation_phases==null)delete copy.periodic_collocation_phases;return copy;};
assert(same(canonicalConfig(recipe.config),canonicalConfig(result.config)),'parent physical config mismatch');
assert(same(recipe.slip_objective,result.slip_objective),'parent slip objective mismatch');
assert.equal(search.termination,'outer_iteration_limit');assert(search.history.length);
let warm=search.continuation;
if(!warm) {
  assert(initialAuditPath!=='-','legacy migration requires independently audited initial state');
  const r=read(initialAuditPath).planning,c=recipe.config;
  let previous=recipe.warm_start?.previous_shifted_norm??Math.max(0,
    r.maximum_force_error_n/c.force_tolerance_n-1,r.maximum_moment_error_nm/c.moment_tolerance_nm-1,
    r.minimum_torque_margin_nm===null?0:-r.minimum_torque_margin_nm/c.torque_tolerance_nm-1,
    (r.maximum_point_penetration_m-c.maximum_point_penetration_m)/c.penetration_scale_m);
  let penalty=recipe.warm_start?.next_penalty??recipe.search.initial_penalty;
  for(const h of search.history) {
    assert.equal(h.penalty,penalty,'legacy penalty sequence mismatch');
    if(h.shifted_constraint_norm>recipe.search.required_reduction*previous) {
      assert(penalty<recipe.search.maximum_penalty,'legacy run should have stopped at penalty cap');
      penalty=Math.min(penalty*recipe.search.penalty_growth,recipe.search.maximum_penalty);
    }
    previous=h.shifted_constraint_norm;
  }
  warm={values:search.values,residuals:search.residuals,multipliers:search.multipliers,
    next_penalty:penalty,previous_shifted_norm:previous,completed_outer_iterations:search.history.at(-1).iteration+1};
}
assert(same(warm.values,search.values)&&same(warm.residuals,search.residuals)&&same(warm.multipliers,search.multipliers),'checkpoint differs from parent final state');
const outer=Number(outerText);assert(Number.isInteger(outer)&&outer>0&&outer<=10000);
const next={...recipe,initial_positions:result.positions,warm_start:warm,
  search:{...recipe.search,maximum_outer_iterations:outer},
  provenance:{parent_recipe:identity(recipePath),parent_result:identity(resultPath),
    initial_audit:initialAuditPath==='-'?null:identity(initialAuditPath),
    operation:'Continue from the exact final motion and stored multipliers, next penalty and shifted-constraint norm. Physical config, point order, bounds, slip objective and inner settings remain unchanged; additional outer iterations have a new explicit per-call budget.',
    legacy_checkpoint_reconstructed:!search.continuation,
    validation:'The shared Rust solver must exactly re-evaluate the stored initial residuals before any resumed optimization. Caller retains model/marker provenance; a local residual match is not a general model identity proof.'}};
fs.writeFileSync(prefix+'.recipe.json',JSON.stringify(next,null,2)+'\n',{flag:'wx'});
const dense=structuredClone(next);dense.config.periodic_collocation_phases=null;dense.config.periodic_cubic_subdivisions=16;
fs.writeFileSync(prefix+'-dense.recipe.json',JSON.stringify(dense,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({prefix,next_penalty:warm.next_penalty,previous_shifted_norm:warm.previous_shifted_norm,completed_outer_iterations:warm.completed_outer_iterations,additional_outer_iterations:outer}));
