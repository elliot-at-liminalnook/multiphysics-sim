// Recipe construction only; Rust performs exact periodic spline knot insertion.
import fs from 'node:fs';import assert from 'node:assert/strict';import {execFileSync} from 'node:child_process';import {createHash} from 'node:crypto';
const [binary,scene,markers,sourcePrefix,outputPrefix,samplesOverride,iterationsText='60',sourceResultPath=sourcePrefix+'.result.json']=process.argv.slice(2);
assert(outputPrefix,'usage: node prepare_refined_periodic_trial.mjs refiner scene markers source_prefix output_prefix [samples_per_interval or -] [iterations] [source_result]');
const read=p=>JSON.parse(fs.readFileSync(p));
const iterations=Number(iterationsText);assert(Number.isInteger(iterations)&&iterations>0);
const override=samplesOverride!==undefined&&samplesOverride!=='-';
const recipe=read(sourcePrefix+'.recipe.json');
const seed=JSON.parse(execFileSync(binary,[scene,markers,sourcePrefix+'.recipe.json',sourceResultPath],{encoding:'utf8'}));
const oldReference=recipe.config.position_reference,n=oldReference.length-1;
assert.equal(recipe.config.periodic_cubic_subdivisions%2,0);
const reference=Array.from({length:2*n+1},(_,k)=>k%2?oldReference[(k-1)/2].map((v,j)=>(v+oldReference[(k+1)/2][j])/2):oldReference[k/2]);
const widths=[0,1].map(j=>({lower:recipe.bounds[1][j].lower-oldReference[1][j],upper:recipe.bounds[1][j].upper-oldReference[1][j]}));
recipe.bounds.slice(0,-1).forEach((row,k)=>row.forEach((bound,j)=>{
  if(j>=2)assert.deepEqual(bound,recipe.bounds[1][j],'this recipe adapter requires constant non-XY bounds');
  else if(k>0)for(const side of ['lower','upper'])assert(Math.abs(bound[side]-oldReference[k][j]-widths[j][side])<1e-12,'this adapter requires a constant XY reference window');
}));
const bounds=reference.slice(0,-1).map((q,k)=>q.map((v,j)=>j<2?(k===0?{lower:seed.positions[0][j],upper:seed.positions[0][j]}:{lower:v+widths[j].lower,upper:v+widths[j].upper}):structuredClone(recipe.bounds[1][j])));
bounds.push(structuredClone(recipe.bounds.at(-1)));
const encoded=seed.positions.slice(0,-1).concat([[seed.positions.at(-1)[0]-seed.positions[0][0],seed.positions.at(-1)[1]-seed.positions[0][1]]]);
const fixedDisplacementRoundoff=[];
// Subtracting the new first control from its translated endpoint can change a
// fixed displacement by a few ulps. Keep it fixed at that representable value,
// record the adjustment, and require a roundoff-sized change (never widen it).
encoded.at(-1).forEach((v,j)=>{
  const b=bounds.at(-1)[j];
  if(b.lower===b.upper&&v!==b.lower){
    const tolerance=8*Number.EPSILON*Math.max(Math.abs(b.lower),Math.abs(seed.positions[0][j]),Math.abs(seed.positions.at(-1)[j]),Number.MIN_VALUE);
    assert(Math.abs(v-b.lower)<=tolerance,'fixed displacement changed beyond roundoff');
    fixedDisplacementRoundoff.push({coordinate:j,previous:b.lower,encoded:v,change:v-b.lower,tolerance});
    b.lower=v;b.upper=v;
  }
});
encoded.forEach((q,k)=>q.forEach((v,j)=>assert(v>=bounds[k][j].lower&&v<=bounds[k][j].upper,`refined seed bound ${k},${j}`)));
recipe.config.position_reference=reference;recipe.config.step_s/=2;recipe.config.periodic_cubic_subdivisions/=2;
if(override){const count=Number(samplesOverride);assert(Number.isInteger(count)&&count>=1&&count<=32);recipe.config.periodic_cubic_subdivisions=count;}
recipe.initial_positions=seed.positions;recipe.bounds=bounds;
const inheritedCheckpoint=recipe.warm_start!==undefined;
if(recipe.search.inner) {
  assert('maximum_outer_iterations' in recipe.search,'nested search requires inequality optimizer controls');
  recipe.search.inner.maximum_iterations=iterations;
  // Knot insertion changes optimizer coordinates. Never pass the stale vector
  // through as a valid checkpoint; a separate native inspection must rebuild it.
  delete recipe.warm_start;
} else recipe.search.maximum_iterations=iterations;
if('smoothing_schedule_m' in recipe)recipe.smoothing_schedule_m=[recipe.config.contact.smoothing_m];
if('stiffness_schedule_n_m' in recipe)recipe.stiffness_schedule_n_m=[recipe.config.contact.stiffness_n_m];
const hash=path=>({path,sha256:createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
recipe.provenance={source_recipe:hash(sourcePrefix+'.recipe.json'),source_result:hash(sourceResultPath),refiner_binary:hash(binary),operation:seed.scope,fixed_displacement_roundoff:fixedDisplacementRoundoff,
  ...(inheritedCheckpoint?{checkpoint:'Inherited checkpoint removed because knot insertion changes parameter layout. Native inequality inspection and explicit row-preserving dual migration are required before a resumed comparison; this preparation alone is not a resumed solve.'}:{}),
  bounds:'Retain existing constant height/orientation/joint bounds and XY window relative to interpolated reference. Anchor first XY control at its exact refined value: this changes the representation gauge, not the initial physical curve. Displacement bounds retained, with any fixed-value encoding roundoff explicitly recorded.',
  sampling:!override?'Double controls, halve control interval and samples per interval; preserve physical collocation times and duration. Independent denser sampling remains required.':`Double controls and halve control interval; explicitly use ${recipe.config.periodic_cubic_subdivisions} samples per new interval. Duration unchanged; independent denser sampling remains required.`,
  solver:iterations+' final-model iterations starting from the retained solution; no softened contact restart or acceptance relaxation.',
  target_speed_m_s:Math.hypot(...recipe.config.velocity_reference.slice(0,2)),limitations:'Local numerical refinement; not hardware bandwidth or a speed maximum.'};
for(const [suffix,data] of [['initial',seed],['recipe',recipe]])fs.writeFileSync(outputPrefix+'.'+suffix+'.json',JSON.stringify(data,null,2)+'\n',{flag:'wx'});
