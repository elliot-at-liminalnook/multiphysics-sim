import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const planPath = `${root}/sdirk-stage-plan.json`, plan = read(planPath);
const statusPath = `${root}/sdirk-stage-status.json`, status = read(statusPath);
assert(status.complete); plan.sources.forEach(verify); status.sources.forEach(verify);
const cases = status.cases.map((c, i) => {
  c.sources.forEach(verify); const spec = plan.cases[i]; assert.equal(c.name, spec.name);
  verify(spec.previous_capture);
  const r = read(c.sources.find(s => s.path.endsWith('.native.json')).path), old = read(spec.previous_capture.path);
  const frames = c => c.frames.map(({stepping_wall_s, ...f}) => f);
  assert.deepEqual(frames(r), frames(old));
  for (const key of ['transitions', 'task', 'contract', 'completed', 'error']) assert.deepEqual(r[key], old[key]);
  const record = structuredClone(r.recording); delete record.config.implicit.newton_audit_window_s;
  assert.deepEqual(record, old.recording);
  const p = read(c.sources.find(s => s.path.endsWith('.profile.json')).path);
  const stages = p.accepted_implicit_steps.filter(d => d.endpoint_audit).map(d => {
    const a = d.endpoint_audit;
    const corrections = a.newton.iterations.flatMap(iteration =>
      (iteration.correction?.largest_unknowns ?? []).map(([column, delta]) => ({
        iteration: iteration.iteration, column, delta, selected_alpha: iteration.line_search?.selected_alpha ?? null})));
    const largest = corrections.reduce((best, c) => !best || Math.abs(c.delta) > Math.abs(best.delta) ? c : best, null);
    const column = r.metadata.coordinate_names.indexOf('joint.-Y | Foot servo output'); assert(column >= 0);
    const joint = r.metadata.joint_indices[column], reduced = a.endpoint_reduced_velocity.length - r.metadata.coordinate_names.length + column;
    return {equation_start_time_s: a.equation_start_time_s, equation_step_s: a.equation_step_s,
      maximum_seed_reduced_speed: Math.max(...a.seed_reduced_velocity.map(Math.abs)),
      maximum_endpoint_reduced_speed: Math.max(...a.endpoint_reduced_velocity.map(Math.abs)),
      maximum_bristle_rate: Math.max(0, ...a.endpoint_bristle_rates.map(Math.abs)),
      observed_foot_servo: {name: r.metadata.coordinate_names[column], seed_rad: a.seed_joint_positions[joint],
        endpoint_rad: a.endpoint_joint_positions[joint], seed_speed_rad_s: a.seed_reduced_velocity[reduced],
        endpoint_speed_rad_s: a.endpoint_reduced_velocity[reduced]},
      iterations: d.nonlinear.iterations, scaled_velocity_residual: d.maximum_scaled_velocity_residual,
      largest_reported_newton_correction: largest};
  });
  assert(stages.length > 0);
  const memoryZero = r.frames.every(f => f.contact_history.every(c => c.bristle_state.every(v => v === 0)));
  return {name: c.name, outcome: c.outcome, exact_previous_physics_and_task: true,
    recorded_contact_memory_zero: memoryZero, floor_friction: r.recording.scene.options.floor_friction, stages};
});
writeFileSync(`${root}/sdirk-stage-summary.json`, JSON.stringify({version: 1, cases,
  sources: [planPath, statusPath, import.meta.filename].map(source),
  scope: 'Auditing exactly preserves both original trajectories. The 20 ms large velocity jump occurs in the second-stage solve at equation start 1.3141421356 s, not in its affine seed. Large Newton corrections precede the distant converged state. This locates the jump but does not prove that another initial guess finds a bounded root. This profile uses regularized Coulomb friction; all sampled contact-memory states and audited bristle rates are zero. No browser, accuracy or hardware qualification.'}, null, 2) + '\n');
console.log(cases.map(c => ({name: c.name, exact: c.exact_previous_physics_and_task, stages: c.stages.length,
  maximum_stage_speed: Math.max(...c.stages.map(s => s.maximum_endpoint_reduced_speed))})));
