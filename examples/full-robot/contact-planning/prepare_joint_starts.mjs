// Enumerate contact-phase starts; all physical evaluation remains in Rust.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const d='examples/full-robot/contact-planning/';
const source=d+'joint-workspace-speed.recipe.json';
const recipe=JSON.parse(fs.readFileSync(source));
const starts=[{id:'reference',candidate:structuredClone(recipe.candidate)}];
assert.equal(recipe.candidate.motion.feet.length,4);
assert.equal(recipe.candidate.motion.feet[0].phase_offset,0);
const phases=[0,.25,.5,.75], duties=[.5,.75];
// A constant mean body pose also avoids tying every new sequence to the old
// trot's body oscillation. It is an initializer, not a prescribed final motion.
const controls=recipe.candidate.motion.body.keyframes.slice(0,-1);
const mean=controls[0].values.map((_,i)=>controls.reduce((s,k)=>s+k.values[i],0)/controls.length);
for(const body of ['reference','constant_mean'])for(const duty of duties)
for(const a of phases)for(const b of phases)for(const c of phases){
  const candidate=structuredClone(recipe.candidate);
  candidate.motion.feet.forEach((f,i)=>{f.phase_offset=[0,a,b,c][i];f.stance_fraction=duty});
  if(body==='constant_mean')candidate.motion.body.keyframes.forEach(k=>k.values=[...mean]);
  starts.push({id:`${body}-d${duty}-p0_${a}_${b}_${c}`,candidate});
}
assert.equal(starts.length,257);
const output=d+'joint-start-screen.batch.json';
fs.writeFileSync(output,JSON.stringify({robot:recipe.robot,starts})+'\n',{flag:'wx'});
const identity=path=>({path,bytes:fs.statSync(path).size,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
fs.writeFileSync(d+'joint-start-screen-preparation.json',JSON.stringify({
  source:identity(source),output:identity(output),starts:starts.length,
  phase_grid:phases,stance_fractions:duties,body_initializers:['reference','constant_mean'],
  speed_m_s:Math.hypot(...recipe.candidate.motion.displacement_world_m)/recipe.candidate.motion.period_s,
  scope:'Systematic initial contact timing screen at the unchanged fast reference speed, placements and swing paths. Foot zero fixes the common phase gauge; all 64 quarter-cycle combinations for the other feet are included at two duty fractions and two body initializations, plus the original reference. Native CAD evaluation initializes force nodes and records every failure. This is neither joint optimization of each start nor exhaustion of continuous timing, body/foot motion, phase counts, or force choices. Low initial residual is only a seed ranking, not a gait certificate. Physical tolerances and robot model are unchanged.'
},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({output,starts:starts.length}));
