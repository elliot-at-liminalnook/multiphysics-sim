// Evidence analysis only. Physics is evaluated by the shared Rust examples.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/contact-implicit/';
const read = name => JSON.parse(fs.readFileSync(root + name));
const trials = ['diagonal25', 'diagonal25-stiffness', 'diagonal25-refinement',
  'diagonal25-scaled', 'diagonal25-unscaled-control'];
const results = trials.map(name => {
  const recipe = read(name + '.recipe.json'), result = read(name + '.result.json');
  const stage = result.stages.at(-1), report = stage.result.report;
  assert.deepEqual(stage.config, recipe.config);
  assert.deepEqual(result.positions[0], recipe.config.position_reference[0]);
  const q = result.positions, duration = (q.length - 1) * recipe.config.step_s;
  const summary = { name, within_planning_tolerances: report.within_planning_tolerances,
    maximum_force_error_n: report.maximum_force_error_n,
    maximum_moment_error_nm: report.maximum_moment_error_nm,
    minimum_torque_margin_nm: report.minimum_torque_margin_nm,
    maximum_point_penetration_m: report.maximum_point_penetration_m,
    planned_mean_diagonal_displacement_rate_m_s:
      (q.at(-1)[0] - q[0][0] + q.at(-1)[1] - q[0][1]) / Math.sqrt(2) / duration,
    thresholded_force_patterns_1n: report.frames.map(f =>
      f.contact_forces_world_n.map(v => v[2] >= 1 ? '1' : '0').join('')),
    cost: stage.result.search.cost, initial_cost: stage.result.search.initial_cost,
    evaluations: stage.result.search.evaluations, termination: stage.result.search.termination };
  if (fs.existsSync(root + name + '.audit.json')) {
    const audit = read(name + '.audit.json');
    assert.deepEqual(audit.planning, report, 'independent audit must reproduce every planning field');
    summary.geometry = { poses: audit.geometry.length,
      maximum_inter_link_penetration_m: Math.max(...audit.geometry.map(f => f.maximum_inter_link_penetration_m)),
      minimum_floor_clearance_m: Math.min(...audit.geometry.flatMap(f => f.floor_clearances.map(p => p.minimum_clearance_m))) };
  }
  return summary;
});
const scaled = read('diagonal25-scaled.recipe.json'), control = read('diagonal25-unscaled-control.recipe.json');
for (const field of ['config', 'initial_positions', 'bounds', 'search', 'smoothing_schedule_m', 'stiffness_schedule_n_m'])
  assert.deepEqual(scaled[field], control[field], `matched comparison ${field}`);
console.log(JSON.stringify({ scope: 'Planning diagnostics only; no measured walking speed or maximum-speed claim.', results }, null, 2));
