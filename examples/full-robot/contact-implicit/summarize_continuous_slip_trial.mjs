import fs from 'node:fs';import assert from 'node:assert/strict';import {isDeepStrictEqual as same} from 'node:util';
import {verifyInequalityTrial,verifyContinuousMeanSlipAudit} from './verify_inequality_evidence.mjs';
const root='examples/full-robot/contact-implicit/',read=n=>JSON.parse(fs.readFileSync(root+n));
const recipe=read('mean68-restore.recipe.json'),control=read('recorded68-restore.recipe.json');
for(const key of ['config','initial_positions','bounds','search'])assert(same(recipe[key],control[key]),'mean trial changed '+key);
const objective=structuredClone(recipe.slip_objective);delete objective.use_continuous_mean_bound;delete objective.loaded_slip_barrier_weight;
assert(same(objective,control.slip_objective),'mean trial changed more than shaping/barrier');
assert(!recipe.warm_start&&!recipe.slip_objective.constrain_loaded_slip,'unexpected checkpoint or hard-cutoff rows');
assert(same(read('mean68-parent.audit.json'),read('barrier68-parent.audit.json')),'default parent audit changed');
const initial=read('mean68-initial.audit.json'),oldInitial=read('warm68-original-initial.audit.json');
for(const key of ['planning','geometry','link_names','scope'])assert(same(initial[key],oldInitial[key]),'initial physical audit changed '+key);
assert(initial.slip.groups.every(g=>g.continuous_mean_slip_upper_bound<recipe.slip_objective.target_ratio),'initial mean bound outside domain');
const result=read('mean68-restore.result.json'),audit=read('mean68-restore.audit.json'),dense=read('mean68-restore-dense.audit.json');
const verified=verifyInequalityTrial(recipe,result,audit,dense),search=verified.search;
const metrics=a=>({...verified.metrics(a),maximum_continuous_mean_slip_bound:Math.max(...a.slip.groups.map(g=>g.continuous_mean_slip_upper_bound)),
  body_path_to_displacement_ratio:a.slip.body_path_m/a.slip.displacement_m});
const checkMean=a=>verifyContinuousMeanSlipAudit(recipe,a);
[initial,audit,dense].forEach(checkMean);
const probes={};
for(const name of ['up','down']) {
  const a=read('mean68-threshold-'+name+'.audit.json'),old=read('barrier68-threshold-'+name+'.audit.json');
  for(const key of ['planning','geometry','scope'])assert(same(a[key],old[key]),'threshold probe physics changed');
  checkMean(a);
  for(const group of a.slip.groups){const prior=old.slip.groups.find(g=>g.group===group.group);
    assert.equal(group.sampled_loaded_slip_ratio,prior.sampled_loaded_slip_ratio);assert.equal(group.rms_slip_upper_bound,prior.rms_slip_upper_bound);}
  probes[name]=a.slip.groups.find(g=>g.group==='-X | Sliding foot crosshead');
}
const actualJump=probes.down.sampled_loaded_slip_ratio-probes.up.sampled_loaded_slip_ratio;
const meanChange=probes.down.continuous_mean_slip_upper_bound-probes.up.continuous_mean_slip_upper_bound;
assert(actualJump>.01&&Math.abs(meanChange)<1e-8,'continuous probe did not resolve the cutoff jump');
console.log(JSON.stringify({scope:'Continuous mean-bound shaping and strict barrier, matched against the recorded seed restoration. Native and independent metric checks retain actual-slip and dense physical gates. No speed gain, runtime qualification or physical maximum is implied.',
  default_parent_exact:true,initial_physics_geometry_exact:true,initial:metrics(initial),initial_slip:initial.slip,
  control_dense:verified.metrics(read('recorded68-restore-dense.audit.json')),
  continuous_coarse:metrics(audit),continuous_dense:metrics(dense),dense_slip:dense.slip,
  cutoff_probe:{probes,actual_jump:actualJump,continuous_mean_change:meanChange},
  planned_projected_45deg_rate_m_s:verified.displacement.reduce((s,v)=>s+v,0)/Math.sqrt(2)/dense.planning.frames.at(-1).time_s,
  search:{termination:search.termination,evaluations:search.evaluations,maximum_violation:search.maximum_violation,within_constraint_tolerance:search.within_constraint_tolerance,
    history:search.history.map(({inner,...h})=>({...h,inner_termination:inner.termination,inner_evaluations:inner.evaluations,rejected_evaluations:inner.rejected_evaluations,last_inner:inner.history.at(-1)})),
    next_penalty:search.continuation?.next_penalty??null}},null,2));
