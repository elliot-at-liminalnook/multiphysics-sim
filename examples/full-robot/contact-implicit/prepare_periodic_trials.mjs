// Solver configuration and numerical initial guesses only. Shared Rust owns
// periodic closure, velocities, dynamics, contact forces and optimization.
import fs from 'node:fs';import assert from 'node:assert/strict';import {createHash} from 'node:crypto';
const root='examples/full-robot/contact-implicit/';
const sourcePath=root+'resolved-work-finegrid-adaptive.recipe.json';
const source=JSON.parse(fs.readFileSync(sourcePath));
const config=structuredClone(source.config);config.periodic_horizontal_translation=true;config.initial_velocity=[];
const reference=config.position_reference,q0=reference[0],intervals=reference.length-1,n=q0.length;
// Every unique pose is free except the initial XY gauge; repeated final pose is
// reconstructed exactly by Rust, with two additional net displacement variables.
const bounds=reference.slice(0,-1).map((q,k)=>q.map((v,j)=>{
  if(k===0&&j<2)return{lower:v,upper:v};
  const old=source.bounds[Math.max(0,k-1)][j];
  const offset=k===0&&j<6?v-reference[1][j]:0;
  return{lower:old.lower+offset,upper:old.upper+offset};
}));
bounds.push([0,1].map(j=>({lower:0,upper:reference.at(-1)[j]-q0[j]+0.12})));
const common={config,bounds,search:{...source.search,maximum_iterations:40,maximum_evaluations:100000},
  smoothing_schedule_m:[0.003,0.001,0.0001,0.00001],stiffness_schedule_n_m:[2000,10000,50000,200000],
  hessian_scaling_exponent:source.hessian_scaling_exponent,derivative_refinement:source.derivative_refinement};
let state=271828183;const random=()=>{state^=state<<13;state^=state>>>17;state^=state<<5;return(state>>>0)/4294967296*2-1;};
const harmonics=Array.from({length:n-6},()=>Array.from({length:3},()=>[random(),random()]));
for(const mode of ['uniform','perturbed']){
  const positions=structuredClone(reference);
  if(mode==='perturbed')for(let k=0;k<intervals;k++)for(let j=6;j<n;j++){
    const width=bounds[k][j].upper-bounds[k][j].lower;
    // Small, deterministic smooth noise breaks phase symmetry. No leg labels,
    // touchdown/swing timing or support sequence enter the numerical seed.
    const perturbation=harmonics[j-6].reduce((s,[a,b],h)=>s+(a*Math.sin(2*Math.PI*(h+1)*k/intervals)+b*Math.cos(2*Math.PI*(h+1)*k/intervals))/(h+1)**2,0)/3;
    positions[k][j]+=0.01*width*perturbation;
  }
  positions[intervals]=positions[0].map((v,j)=>j<2?v+reference.at(-1)[j]-q0[j]:v);
  const encoded=positions.slice(0,-1).concat([[positions.at(-1)[0]-positions[0][0],positions.at(-1)[1]-positions[0][1]]]);
  encoded.forEach((q,k)=>q.forEach((v,j)=>assert(v>=bounds[k][j].lower&&v<=bounds[k][j].upper)));
  const recipe={...structuredClone(common),initial_positions:positions,provenance:{
    source_recipe:sourcePath,source_recipe_sha256:createHash('sha256').update(fs.readFileSync(sourcePath)).digest('hex'),
    operation:'Translate-periodic whole-body orbit: optimize all unique poses and net XY travel; exact final height/orientation/joint closure and wrapped boundary velocity in shared Rust. Initial pose free except XY gauge. No initial-rest assumption or prescribed contact schedule.',
    target_speed_m_s:Math.hypot(...config.velocity_reference.slice(0,2)),period_s:intervals*config.step_s,
    initial_guess:mode,seed:mode==='perturbed'?271828183:null,
    perturbation:mode==='perturbed'?'Three zero-mean Fourier harmonics with seeded independent coefficients for every actuator coordinate, at at most 1% of its software bound width. Numerical symmetry breaking only; all poses remain optimization variables.':null,
    continuation:'Same stiffness/smoothing continuation for both seeds, finishing at the resolved detailed-planning parameters. Other physical properties, objectives and physical acceptance tolerances retained.',
    limitations:'Local finite-period search, not a global speed bound. Geometric collision checks, detailed runtime orbit entry/tracking, stable repetition and WASD qualification remain separate. The existing at-rest compiler rejects periodic orbits.'}};
  fs.writeFileSync(root+'periodic-'+mode+'.recipe.json',JSON.stringify(recipe,null,2)+'\n');
  fs.writeFileSync(root+'periodic-'+mode+'.initial.json',JSON.stringify({positions})+'\n');
}
console.log({unique_knots:intervals,period_s:intervals*config.step_s,free_variables:bounds.flat().filter(b=>b.upper>b.lower).length,mode:'exact translating periodic boundary'});
