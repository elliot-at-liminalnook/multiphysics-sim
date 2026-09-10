import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const [stem] = process.argv.slice(2);
assert(stem && !stem.endsWith('.json'));
const read = p => JSON.parse(fs.readFileSync(p));
const identity = p => ({path:p,sha256:crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex')});
const recipe = read(stem+'.recipe.json'), result = read(stem+'.result.json');
const initial = read(stem+'-initial.result.json');
assert.equal(result.search.values.length,recipe.variables.length);
assert.equal(initial.constraints.inequalities.length,result.report.constraints.inequalities.length);
const get = (c,d) => {
  if(d.kind==='force')return c.force_templates[d.clock][d.foot].keyframes[d.node].values[d.axis];
  const m=c.motion;d=d.decision;
  switch(d.kind){
    case 'period':return m.period_s;
    case 'displacement_along_direction':return m.displacement_world_m.reduce((s,v,i)=>s+v*recipe.robot.direction_world[i],0);
    case 'displacement':return m.displacement_world_m[d.axis];
    case 'foot_phase':return m.feet[d.foot].phase_offset;
    case 'foot_stance':return m.feet[d.foot].stance_fraction;
    case 'foot_center':return m.feet[d.foot].center_world_m[d.axis];
    case 'foot_swing':return m.feet[d.foot].swing_offset_world_m[d.axis];
    case 'body_control':return m.body.keyframes[d.control].values[d.channel];
    default:throw Error('unknown decision');
  }
};
const groups={};
for(const v of recipe.variables){
  const key=v.decision.kind==='force'?'force':v.decision.decision.kind;
  const delta=Math.abs(get(result.candidate,v.decision)-get(recipe.candidate,v.decision));
  const g=groups[key]??={variables:0,changed_over_1e_9:0,maximum_normalized_change:0};
  g.variables++;g.changed_over_1e_9+=Number(delta>1e-9);
  g.maximum_normalized_change=Math.max(g.maximum_normalized_change,delta/(v.bound.upper-v.bound.lower));
}
const metrics=r=>({speed_m_s:r.motion_report.speed_m_s,force_error_n:r.motion_report.maximum_force_error_n,
  moment_error_nm:r.motion_report.maximum_moment_error_nm,torque_margin_nm:r.motion_report.minimum_torque_margin_nm,
  cone_violation_n:r.maximum_cone_violation_n,
  maximum_inequality:Math.max(0,...r.constraints.inequalities),sampled_feasible:r.sampled_feasible});
const output={inputs:['.recipe.json','-initial.result.json','.result.json'].map(s=>identity(stem+s)),
  initial:metrics(initial),final:metrics(result.report),variable_groups:groups,
  solver:{evaluations:result.search.evaluations,termination:result.search.termination,
    outer_iterations:result.search.history.length,history:result.search.history.map(h=>({iteration:h.iteration,
      objective_cost:h.objective_cost,maximum_violation:h.maximum_violation,
      accepted_inner_steps:h.inner.history.filter(i=>i.accepted).length}))},
  retained_sampled_feasible:result.best_sampled_feasible!==null,
  scope:'Offline joint optimization evidence only. Every physical inequality must pass independent validation before runtime qualification. A lower violation or changed variables does not constitute a faster gait or a physical speed ceiling.'};
fs.writeFileSync(stem+'.summary.json',JSON.stringify(output,null,2)+'\n');
console.log(JSON.stringify({initial:output.initial,final:output.final,solver:output.solver,groups}));
