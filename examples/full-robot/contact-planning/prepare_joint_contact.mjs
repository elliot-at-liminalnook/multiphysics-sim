// Configuration only: all force evaluation, kinematics and optimization run in Rust.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const d = 'examples/full-robot/contact-planning';
const source = `${d}/return-x25-reference.result.json`;
const boundsSource = `${d}/heading-plus45-restoration.recipe.json`;
const scenePath = 'runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json';
const read = p => JSON.parse(fs.readFileSync(p));
const identity = p => ({path:p, sha256:crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex')});
const sourceData = read(source), scene = read(scenePath), robot = structuredClone(sourceData.recipe);
const motion = structuredClone(sourceData.motion);
assert.equal(robot.expected_cad_sha256, scene.robot.source.cad_sha256);
assert(scene.robot.gravity[2] < 0 && scene.robot.gravity[0] === 0 && scene.robot.gravity[1] === 0);
const weight = scene.robot.links.filter(l => !l.ground).reduce((s,l) => s + l.mass, 0) * -scene.robot.gravity[2];
// This is an explicit first search mesh, not the retained independent dense audit.
robot.uniform_samples = 16;
robot.additional_phases = [];
robot.sampling = 'contact_intervals';
robot.include_body_knots = true;
robot.additional_clocks = [{phase_rate:-1, phase_acceleration_per_s:0}];
robot.target_speed_m_s = 0.30;
robot.speed_residual_scale_m_s = 0.05;
const templateTimes = [0, 0.05, 0.25, 0.5, 0.75, 0.95, 1];
// CAD weight provides an initializer, not a fixed load sharing constraint.
const force_templates = [0,1].map(() => motion.feet.map(() => ({interpolation:'linear',
  keyframes:templateTimes.map((time_s,i) => ({time_s, values:[0,0,i===0||i===templateTimes.length-1?0:weight/2]}))})));
const variables = read(boundsSource).variables.map(v => {
  const copy = structuredClone(v);
  if(copy.decision.kind === 'foot_center') {
    const {foot,axis} = copy.decision;
    const center = motion.feet[foot].center_world_m[axis];
    copy.bound = {lower:center-0.02,upper:center+0.02};
  }
  if(copy.decision.kind === 'displacement_along_direction') copy.bound.upper = 0.14;
  return {decision:{kind:'motion',decision:copy.decision},bound:copy.bound};
});
for(let clock=0;clock<2;clock++) for(let foot=0;foot<motion.feet.length;foot++)
  for(let node=1;node<templateTimes.length-1;node++) for(let axis=0;axis<3;axis++)
    variables.push({decision:{kind:'force',clock,foot,node,axis},
      bound:{lower:axis===2?0:-weight,upper:weight}});
const recipe = {robot,candidate:{motion,force_templates},variables,search:{
  maximum_outer_iterations:3,maximum_evaluations:1400,initial_penalty:1,maximum_penalty:10000,
  penalty_growth:10,required_reduction:0.25,constraint_tolerance:1e-7,
  complementarity_tolerance:1e-6,scaling_exponent:0,
  inner:{maximum_iterations:4,maximum_evaluations:500,difference_step:0.0001,
    initial_damping:0.1,gradient_tolerance:0.00001}}};
const output = `${d}/joint-x25.recipe.json`;
fs.writeFileSync(output,JSON.stringify(recipe,null,2)+'\n',{flag:'wx'});
fs.writeFileSync(`${d}/joint-x25-preparation.json`,JSON.stringify({
  inputs:[source,boundsSource,scenePath].map(identity),output:identity(output),weight_n:weight,
  variables:variables.length,motion_variables:variables.filter(v=>v.decision.kind==='motion').length,
  force_variables:variables.filter(v=>v.decision.kind==='force').length,
  scope:'First joint-force search from measured .234 m/s runtime candidate; nominal reference .211710 m/s. All prior motion variable groups plus independent forward/reverse force nodes are free. Target .30 m/s and search intervals are experimental, not theoretical physical bounds. Original actuator, contact and geometry model and physical tolerances retained; overlap explicitly must be zero. Coarse search requires independent dense validation; no gait promotion.'
},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({output,variables:variables.length,weight_n:weight}));
