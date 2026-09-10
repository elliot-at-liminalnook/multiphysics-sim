// Robot configuration only; shared Rust samples references and evaluates physics.
import fs from 'node:fs';import crypto from 'node:crypto';import {spawnSync} from 'node:child_process';import assert from 'node:assert/strict';
const d='examples/full-robot/contact-implicit',source='examples/full-robot/contact-planning/diagonal21-reference.result.json';
const read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const r=read(source),intervals=12,step=r.motion.period_s/intervals;
const samplePath=d+'/diagonal21-seed.json';assert(!fs.existsSync(samplePath));
const sample=spawnSync('/Users/elliot/physics-simulator/target/gait-exploration/release/examples/sample_contact_reference',[source,String(r.initial_phase_s),String(step),String(intervals)],{encoding:'utf8'});assert.equal(sample.status,0,sample.stderr);fs.writeFileSync(samplePath,sample.stdout,{flag:'wx'});const seed=JSON.parse(sample.stdout);
const targetSpeed=.25,direction=r.recipe.direction_world,n=seed.positions[0].length;
const reference=structuredClone(seed.positions);for(let k=1;k<reference.length;k++)for(let i=0;i<3;i++)reference[k][i]+=direction[i]*(targetSpeed-r.nominal_speed_m_s)*k*step;
const scene='runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json';
const config={expected_cad_sha256:r.recipe.expected_cad_sha256,independent_coordinates:r.recipe.independent_coordinates,
 embedding:r.recipe.embedding,actuators:r.recipe.actuators,
 contact:{stiffness_n_m:2000,smoothing_m:.001,dissipation_velocity_m_s:.2,friction_coefficient:read(scene).robot.world.floor_friction,stiction_velocity_m_s:.02},
 step_s:step,initial_velocity:seed.initial_velocity,position_reference:reference,
 position_scales:[.3,.3,.03,.15,.15,.2,...Array(n-6).fill(100)],velocity_reference:[...direction.map(v=>v*targetSpeed),0,0,0,...Array(n-6).fill(0)],
 velocity_scales:[.05,.05,.2,.3,.3,.3,...Array(n-6).fill(30)],force_tolerance_n:.05,moment_tolerance_nm:.02,
 torque_tolerance_nm:.01,torque_effort_scale_nm:30,maximum_point_penetration_m:.001,penetration_scale_m:.001};
const bounds=reference.slice(1).map(q=>q.map((v,i)=>i<6?{lower:v-(i<3?.06:.3),upper:v+(i<3?.06:.3)}:{lower:r.recipe.joint_search_bounds[i-6].lower,upper:r.recipe.joint_search_bounds[i-6].upper}));
for(let k=1;k<seed.positions.length;k++)for(let i=0;i<n;i++)assert(seed.positions[k][i]>=bounds[k-1][i].lower&&seed.positions[k][i]<=bounds[k-1][i].upper,'seed bounds');
const recipe={config,initial_positions:seed.positions,bounds,search:{maximum_iterations:8,maximum_evaluations:4000,difference_step:1e-5,initial_damping:.1,gradient_tolerance:1e-5},smoothing_schedule_m:[.01,.003,.001],
 provenance:{source_reference:source,source_sha256:sha(source),seed_file:samplePath,seed_sha256:sha(samplePath),scene,scene_sha256:sha(scene),target_speed_m_s:targetSpeed,
 physical_definition:'CAD geometry, masses, inertias, joints and effective actuator estimates retained; friction copied from the explicit runtime model.',
 planning_approximation:'IDTO equations 3-6, one declared CAD foot point per leg on a flat plane. 2000 N/m stiffness follows the published quadruped planning scale, not CAD runtime stiffness. Smoothing .01/.003/.001 m, dissipation velocity .2 m/s, stiction .02 m/s are explicit numerical/model assumptions.',
 objective:'Track .25 m/s along +45 degrees with weak joint posture/rate regularization. All body/joint knots after the fixed initial state are free within explicit numerical/software bounds. Existing gait is only an initial guess, with no contact timing, sequence, stance fraction or periodicity constraints.',
 limitations:'Finite horizon; no periodicity or terminal viability guarantee, interlink/extended-surface contact checks or actual runtime tracking yet. Weighted penalties are not the published equality-constrained IDTO solver. No theoretical maximum claim.'}};
fs.writeFileSync(d+'/diagonal25.recipe.json',JSON.stringify(recipe,null,2)+'\n',{flag:'wx'});console.log({intervals,variables:intervals*n,step_s:step,target_speed_m_s:targetSpeed});
