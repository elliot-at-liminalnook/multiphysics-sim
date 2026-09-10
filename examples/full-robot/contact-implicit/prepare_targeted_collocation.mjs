// Select numerical check times from shared-Rust residual evidence, not leg phases.
import fs from 'node:fs';import assert from 'node:assert/strict';import {createHash} from 'node:crypto';
const [sourcePrefix,densePath,outputPrefix,countText='16',iterationsText='60']=process.argv.slice(2);
assert(outputPrefix,'usage: node prepare_targeted_collocation.mjs source_prefix dense_audit output_prefix [additional_points] [iterations]');
const read=p=>JSON.parse(fs.readFileSync(p));const recipe=read(sourcePrefix+'.recipe.json'),result=read(sourcePrefix+'.result.json'),dense=read(densePath);
const count=Number(countText),iterations=Number(iterationsText);assert(Number.isInteger(count)&&count>0);assert(Number.isInteger(iterations)&&iterations>0);
const c=recipe.config,n=(c.position_reference.length-1)*c.periodic_cubic_subdivisions;
assert(c.periodic_horizontal_translation&&Number.isInteger(n)&&n>0,'analytic periodic cubic recipe required');
assert([c.force_tolerance_n,c.moment_tolerance_nm,c.torque_tolerance_nm,c.maximum_point_penetration_m].every(v=>Number.isFinite(v)&&v>0),'positive finite physical gates required for normalized peak selection');
const period=(c.position_reference.length-1)*c.step_s;
const oldPhases=c.periodic_collocation_phases??Array.from({length:n},(_,k)=>(k+1)/n);
assert(oldPhases.length>1&&oldPhases.at(-1)===1&&oldPhases.every((v,i)=>Number.isFinite(v)&&v>0&&(i===0||v>oldPhases[i-1])),'sorted valid current grid required');
const phases=new Set(oldPhases),frameCount=dense.planning.frames.length;
assert(frameCount>oldPhases.length,'a denser independent audit is required');
assert(dense.planning.frames.every((f,i)=>Math.abs(f.time_s-(i+1)/frameCount*period)<1e-12*Math.max(1,period)),'uniform dense audit at the same period required');
const score=f=>Math.max(...f.unactuated_wrench.slice(0,3).map(v=>Math.abs(v)/c.force_tolerance_n),
  ...f.unactuated_wrench.slice(3).map(v=>Math.abs(v)/c.moment_tolerance_nm),
  Math.max(0,-(f.minimum_torque_margin_nm??0))/c.torque_tolerance_nm,
  ...f.gaps_m.map(v=>Math.max(0,-v)/c.maximum_point_penetration_m));
const worst=new Map();let cell=0;
for(let i=0;i<frameCount;i++) {
  const phase=(i+1)/frameCount,f=dense.planning.frames[i];
  while(phase>oldPhases[cell])cell++;
  if(phases.has(phase))continue;
  const entry={cell,phase,score:score(f),time_s:f.time_s},previous=worst.get(cell);
  if(entry.score>1&&(!previous||entry.score>previous.score))worst.set(cell,entry);
}
const selected=[...worst.values()].sort((a,b)=>b.score-a.score||a.phase-b.phase).slice(0,count);
assert(selected.length>0,'no additional violating collocation samples found');
selected.forEach(p=>phases.add(p.phase));
c.periodic_collocation_phases=[...phases].sort((a,b)=>a-b);
recipe.initial_positions=result.positions;recipe.search.maximum_iterations=iterations;
if('smoothing_schedule_m' in recipe)recipe.smoothing_schedule_m=[c.contact.smoothing_m];
if('stiffness_schedule_n_m' in recipe)recipe.stiffness_schedule_n_m=[c.contact.stiffness_n_m];
const hash=path=>({path,sha256:createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
recipe.provenance={source_recipe:hash(sourcePrefix+'.recipe.json'),source_result:hash(sourcePrefix+'.result.json'),dense_audit:hash(densePath),
  operation:'Retain every existing time and add the largest missed normalized balance, torque or penetration violation from each of the worst intervals. Fix this numerical grid during optimization; no contact schedule, physical model, bounds, target or gate is changed.',
  quadrature:'Physical work and full-task costs retain preceding-interval time quadrature. Feasibility restoration, when selected by its recipe, weights each checked physical sample equally.',selected,
  prior_samples:oldPhases.length,new_samples:c.periodic_collocation_phases.length,period_s:period,target_speed_m_s:Math.hypot(...c.velocity_reference.slice(0,2)),
  limitations:'Finite local solve. Independent dense auditing must catch peaks that move elsewhere; collision and slip acceptance remain separate.'};
for(const [suffix,data] of [['initial',{positions:result.positions}],['recipe',recipe]])fs.writeFileSync(outputPrefix+'.'+suffix+'.json',JSON.stringify(data,null,2)+'\n',{flag:'wx'});
console.log({prior_samples:oldPhases.length,new_samples:phases.size,selected:selected.slice(0,3)});
