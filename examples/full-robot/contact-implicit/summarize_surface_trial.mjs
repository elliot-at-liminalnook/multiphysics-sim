// Evidence reduction only; all forces and geometry come from shared Rust audits.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const root = 'examples/full-robot/contact-implicit/';
const name = process.argv[2] ?? 'surface25';
assert(/^[a-z0-9-]+$/.test(name));
const read = file => JSON.parse(fs.readFileSync(root + file));
const markers = read('surface-markers.json').markers;
const links = [...new Set(markers.map(p => p.link))];
const result = read(name + '.result.json');
const recipe = read(name + '.recipe.json');
const audit = read(name + '.audit.json');
assert(isDeepStrictEqual(audit.planning, result.stages.at(-1).result.report),'Independent audit differs from optimization report');
if(recipe.config.periodic_horizontal_translation) {
  assert.deepEqual(result.positions.at(-1).slice(2),result.positions[0].slice(2));
  assert.deepEqual(audit.planning.periodic_boundary.initial_velocity,audit.planning.frames.at(-1).velocity);
} else assert.deepEqual(result.positions[0], recipe.config.position_reference[0]);
const initial = read('surface25-initial.audit.json');
assert(initial.geometry.every(f => f.maximum_inter_link_penetration_m === 0));
assert(initial.geometry.every(f => f.floor_clearances.every(p => p.minimum_clearance_m >= .00002-1e-12)));
const geometrySummary = geometry => ({poses:geometry.length,
  maximum_inter_link_penetration_m:Math.max(...geometry.map(f => f.maximum_inter_link_penetration_m)),
  minimum_floor_clearance_m:Math.min(...geometry.flatMap(f => f.floor_clearances.map(p => p.minimum_clearance_m)))});
const stages = result.stages.map(stage => {
  const r = stage.result.report, controls = stage.result.positions;
  const q = r.periodic_boundary?.initial_position ? [r.periodic_boundary.initial_position,...r.frames.map(f=>f.position)] : controls;
  const dt = stage.config.step_s/(stage.config.periodic_cubic_subdivisions??1);
  const weights=r.frames.map((f,k)=>stage.config.periodic_collocation_phases?f.time_s-(k?r.frames[k-1].time_s:0):dt);
  const duration=stage.config.periodic_collocation_phases?r.frames.at(-1).time_s:(q.length-1)*dt;
  assert(r.frames.every(f => f.gaps_m.length === markers.length));
  const normalForces = r.frames.map(f => links.map(link => f.contact_forces_world_n
    .reduce((sum, value, i) => sum + (markers[i].link === link ? value[2] : 0), 0)));
  const bodyPath=q.slice(1).reduce((sum,p,k)=>sum+Math.hypot(p[0]-q[k][0],p[1]-q[k][1]),0);
  let work=0;
  const loadedPaths=links.map(link=>r.frames.reduce((sum,f,k)=>{
    const dt=weights[k];
    let load=0,weightedSpeed=0;
    for(let i=0;i<markers.length;i++)if(markers[i].link===link){
      const force=f.contact_forces_world_n[i],v=f.contact_velocities_world_m_s[i];
      load+=force[2];weightedSpeed+=force[2]*Math.hypot(v[0],v[1]);
      work-=dt*(force[0]*v[0]+force[1]*v[1]);
    }
    return sum+(load>=1?dt*weightedSpeed/load:0);
  },0));
  return {periodic_boundary:r.periodic_boundary, contact:stage.config.contact, within_planning_tolerances:r.within_planning_tolerances,
    maximum_force_error_n:r.maximum_force_error_n, maximum_moment_error_nm:r.maximum_moment_error_nm,
    minimum_torque_margin_nm:r.minimum_torque_margin_nm, maximum_surface_sample_penetration_m:r.maximum_point_penetration_m,
    planned_mean_diagonal_body_displacement_rate_m_s:(q.at(-1)[0]-q[0][0]+q.at(-1)[1]-q[0][1])/Math.sqrt(2)/duration,
    planned_terminal_diagonal_body_velocity_m_s:(r.frames.at(-1).velocity[0]+r.frames.at(-1).velocity[1])/Math.sqrt(2),
    dissipated_sliding_work_j:work,
    maximum_planned_loaded_slip_ratio:bodyPath>0?Math.max(...loadedPaths)/bodyPath:null,
    actuator_work_diagnostics:r.periodic_boundary?stage.config.independent_coordinates.map((name,j)=>{
      const powers=r.frames.map(f=>f.motor_torques_nm[j]*f.velocity[6+j]);
      const motor=stage.config.actuators[name];
      return{name,maximum_abs_speed_rad_s:Math.max(...r.frames.map(f=>Math.abs(f.velocity[6+j]))),
        maximum_motoring_power_w:Math.max(0,...powers),maximum_braking_power_w:-Math.min(0,...powers),
        signed_discrete_work_j:stage.config.periodic_collocation_phases?powers.reduce((s,v,k)=>s+weights[k]*v,0):dt*powers.reduce((s,v)=>s+v,0),
        positive_discrete_work_j:stage.config.periodic_collocation_phases?powers.reduce((s,v,k)=>s+weights[k]*Math.max(0,v),0):dt*powers.reduce((s,v)=>s+Math.max(0,v),0),
        optimistic_peak_motoring_power_w:motor.stall_torque*motor.no_load_speed/4,
        power_bound_formula:'stall_torque * no_load_speed / 4; same effective-servo model, no thermal claim'};
    }):undefined,
    independent_coordinate_ranges:stage.config.independent_coordinates.map((name,i) => ({name,
      minimum:Math.min(...q.map(p => p[6+i])),maximum:Math.max(...q.map(p => p[6+i]))})),
    per_foot_normal_forces_n:normalForces,
    thresholded_foot_force_patterns_1n:normalForces.map(f => f.map(v => v>=1?'1':'0').join('')),
    search:{initial_cost:stage.result.search.initial_cost,cost:stage.result.search.cost,
      evaluations:stage.result.search.evaluations,termination:stage.result.search.termination}};
});
const stationary = read('surface25.recipe.json'), translating = read('surface25-translating-seed.recipe.json');
for (const field of ['config','bounds','search','smoothing_schedule_m','stiffness_schedule_n_m','hessian_scaling_exponent'])
  assert.deepEqual(stationary[field],translating[field],`matched initial-guess comparison: ${field}`);
console.log(JSON.stringify({scope:'Finite-horizon planning diagnostics; no periodic gait, detailed runtime tracking or physical maximum claim.',
  links, contact_samples:markers.length, initial_geometry:geometrySummary(recipe.config.periodic_horizontal_translation?[audit.geometry[0]]:initial.geometry),
  final_geometry:geometrySummary(audit.geometry), stages},null,2));
