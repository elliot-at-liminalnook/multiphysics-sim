// Closed-loop composite acquisition. Rust proposes values and predicts motion;
// each completed physical prefix supplies the next model's measured responses.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {loadExperiment} from './run_affine_speed_search.mjs';
import {read,write,pin,execute,validateScreen,evaluatePlanarCandidate,bindPlanarResponses} from './planar_speed_experiment.mjs';

const specPath = process.argv[2]; assert(specPath,'usage: run_composite_planar_speed specification.json');
const spec = read(specPath), seed = read(spec.seed_request), seedScores = read(spec.seed_scores);
const screen = read(spec.screen_spec), sourceSpec = read(screen.source_experiment), source = loadExperiment(sourceSpec);
assert.equal(spec.version,1);
assert(Number.isSafeInteger(spec.iterations) && spec.iterations > 0);
assert(Number.isSafeInteger(spec.proposal_seed) && spec.proposal_seed >= 0);
assert(Number.isFinite(spec.local_radius) && spec.local_radius > 0 && spec.local_radius <= 1);
assert([spec.local_candidates,spec.global_candidates].every(x => Number.isSafeInteger(x) && x > 0));
assert.equal(seed.prefix_s,screen.prefix_s); assert.equal(seed.horizon_s,source.duration);
const controllerDimensions = sourceSpec.problem.parameters.length;
const stepParameter = spec.physics_step_parameter;
const fixed = stepParameter ? [{name:stepParameter,value:source.config.step_s}] : [];
if (stepParameter) {
  assert.deepEqual(seed.problem.parameters.slice(0,controllerDimensions),sourceSpec.problem.parameters);
  assert.equal(seed.problem.parameters.length,controllerDimensions+1);
  const p = seed.problem.parameters.at(-1);
  assert.equal(p.name,stepParameter); assert.equal(p.unit,'s');
  assert(source.config.step_s >= p.bounds[0] && source.config.step_s <= p.bounds[1]);
  assert.deepEqual(seed.config.incumbent_conditions,fixed);
} else assert.deepEqual(seed.problem.parameters,sourceSpec.problem.parameters);
assert.equal(seed.windows.length,screen.fit_windows.length);
assert.equal(seedScores.length,seed.observations.length);
validateScreen(source,screen);
for (const p of read(spec.seed_context).files) assert.equal(pin(p.path).sha256,p.sha256,'changed seed dependency: '+p.path);
const templateReplay = read(spec.template_replay);
assert.deepEqual(templateReplay.task,source.task);
const root = spec.output_directory; fs.mkdirSync(root); write(root+'/spec.json',spec);
const dependencies = [specPath,spec.seed_request,spec.seed_scores,spec.seed_context,spec.screen_spec,
  spec.template_replay,spec.selector,spec.predictor,screen.runtime,screen.source_experiment,
  sourceSpec.scene,sourceSpec.config,sourceSpec.task,sourceSpec.actions,sourceSpec.reference,
  sourceSpec.affine_binary,...(sourceSpec.command_schema?[sourceSpec.command_schema]:[]),
  import.meta.filename,new URL('./planar_speed_experiment.mjs',import.meta.url).pathname,
  new URL('./run_affine_speed_search.mjs',import.meta.url).pathname];
const context = {version:1,files:[...new Set(dependencies)].map(pin),
  scope:'Measured prefix trajectory responses feed adaptive composite EI. Optional physics timestep is an explicit model coordinate, fixed to the executing runtime timestep for queries and incumbent selection. Full-episode scores never enter this forecast objective. Global proposals use the entire declared controller domain, which is not a physical speed bound. STOP cancels between physical evaluations.'};
const contextId = crypto.createHash('sha256').update(JSON.stringify(context)).digest('hex');
write(root+'/context.json',{...context,context_id:contextId});
const problem = {...seed.problem,context_id:contextId};
const observations = seed.observations.map(o => ({...o,context_id:contextId}));
const scores = [...seedScores], rows = [];
for (let i=0;i<spec.iterations && !fs.existsSync(root+'/STOP');i++) {
  const best = scores.reduce((index,score,j) => score !== null
    && (!stepParameter || observations[j].values.at(-1) === source.config.step_s)
    && (index === null || score < scores[index]) ? j : index,null);
  assert(best !== null,'a completed seed score is required');
  const local = problem.parameters.map((p,j) => {
    if (stepParameter && j === controllerDimensions) return [0,1];
    const z = (observations[best].values[j]-p.bounds[0])/(p.bounds[1]-p.bounds[0]);
    return [Math.max(0,z-spec.local_radius),Math.min(1,z+spec.local_radius)];
  });
  const proposalRoot = root+'/proposal-'+String(i).padStart(3,'0'); fs.mkdirSync(proposalRoot);
  const request = {...seed,problem,observations,candidates:[],
    regions:[{bounds:local,count:spec.local_candidates,seed:spec.proposal_seed+3*i,...(stepParameter?{fixed}:{})},
      {bounds:problem.parameters.map(() => [0,1]),count:spec.global_candidates,seed:spec.proposal_seed+3*i+1,...(stepParameter?{fixed}:{})}],
    config:{...seed.config,seed:spec.proposal_seed+3*i+2}};
  write(proposalRoot+'/request.json',request);
  assert.equal((await execute(proposalRoot,'suggest',spec.selector,
    [proposalRoot+'/request.json',proposalRoot+'/result.json'])).exit_code,0);
  const result = read(proposalRoot+'/result.json');
  assert.equal(result.observed_objectives.length,scores.length);
  result.observed_objectives.forEach((score,j) => {
    if (scores[j] === null) assert.equal(score,null);
    else assert(Number.isFinite(score) && Math.abs(score-scores[j]) <= 1e-12,'Rust forecast composition changed at row '+j);
  });
  write(proposalRoot+'/composition-check.json',{passed:true,rows:scores.length,absolute_tolerance:1e-12});
  const selected = result.result.selected;
  if (stepParameter) {
    assert.equal(selected.values.length,controllerDimensions+1);
    assert.equal(selected.values.at(-1),source.config.step_s);
    assert.equal(observations[result.result.observed_best_index].values.at(-1),source.config.step_s);
  }
  const path = root+'/evaluation-'+String(i).padStart(3,'0');
  const evaluation = await evaluatePlanarCandidate({sourceSpec,source,template:templateReplay.runtime,
    screen,predictor:spec.predictor,path,values:selected.values.slice(0,controllerDimensions),ordinal:i});
  const row = {...evaluation.row,selection_method:'adaptive composite expected improvement',
    proposal:proposalRoot+'/result.json',posterior_objective_at_mean:selected.objective_at_mean,
    posterior_expected_improvement:selected.expected_improvement,
    ...(stepParameter?{model_values:selected.values,physics_step_s:source.config.step_s}:{})};
  const complete = evaluation.outcome.status === 'complete';
  const responses = complete ? bindPlanarResponses(evaluation.fits,seed.windows,seed.responses.length) : null;
  const outcome = complete ? {status:'complete',responses} : evaluation.outcome;
  if (complete) write(path+'/prediction-errors.json',seed.responses.map((r,j) => ({...r,
    actual:responses[j],mean:selected.response_mean[j],std:selected.response_std[j],
    error:responses[j]-selected.response_mean[j]})));
  write(path+'/summary.json',row);
  const observation = {context_id:contextId,values:selected.values,outcome,evidence:path+'/summary.json'};
  write(path+'/observation.json',observation);
  observations.push(observation); scores.push(complete ? evaluation.outcome.objective : null); rows.push(row);
  write(root+`/iteration-${i}.json`,{rows,observations,scores}); console.log(JSON.stringify(row));
}
write(root+'/result.json',{version:1,problem,observations,scores,rows,stopped:fs.existsSync(root+'/STOP'),
  scope:'Adaptive composite search over measured short-prefix responses. Neither forecast speed nor prefix survival qualifies sustained speed. Independent response GPs and finite candidate pools do not prove uncertainty calibration, sample efficiency or gait-space exhaustion.'});
