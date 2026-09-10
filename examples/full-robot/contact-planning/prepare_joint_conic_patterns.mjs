import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import crypto from 'node:crypto';
const d='examples/full-robot/contact-planning/';
const read=n=>JSON.parse(fs.readFileSync(d+n));
const base=read('joint-timed-speed.recipe.json'),warm=read('joint-conic-warm-final.recipe.json');
assert(isDeepStrictEqual(base.robot,warm.robot),'Control robot recipes differ');
const forceBounds=base.candidate.force_templates.map(clock=>clock.map(()=>[null,null,null]));
for(const v of base.variables.filter(v=>v.decision.kind==='force')){const {clock,foot,axis}=v.decision;const old=forceBounds[clock][foot][axis];if(old)assert(isDeepStrictEqual(old,v.bound),'Force bounds vary by node');forceBounds[clock][foot][axis]=v.bound;}
assert(forceBounds.flat(2).every(b=>b&&Number.isFinite(b.lower)&&Number.isFinite(b.upper)&&b.lower<=b.upper));
const conic={solver:{maximum_iterations:200,tolerance:1e-9},force_bounds:forceBounds};
const starts=[{id:'control-fast-original-basis',candidate:base.candidate,align_and_bind_forces:false},{id:'control-warm-feasible',candidate:warm.candidate,align_and_bind_forces:false}];
// The warm control's motion comes from the failed native search; its forces are
// optimized afresh, reproducing the independently feasible convex result.
const count=base.candidate.motion.body.keyframes.length-1;
const mean=Array.from({length:6},(_,axis)=>base.candidate.motion.body.keyframes.slice(0,count).reduce((sum,k)=>sum+k.values[axis],0)/count);
const grid=[0,.25,.5,.75],duties=[.6,.75],jitter=[0,.0005,.001,.0015];
for(const body of ['original','mean'])for(const duty of duties)for(let a=0;a<4;a++)for(let b=0;b<4;b++)for(let c=0;c<4;c++){
 const candidate=structuredClone(base.candidate);candidate.force_timing=null;
 const phases=[0,grid[a]+jitter[1],grid[b]+jitter[2],grid[c]+jitter[3]];
 candidate.motion.feet.forEach((f,i)=>{f.phase_offset=phases[i];f.stance_fraction=duty;});
 if(body==='mean')candidate.motion.body.keyframes.forEach(k=>{k.values=[...mean];});
 candidate.force_templates=candidate.force_templates.map(clock=>clock.map(()=>({interpolation:'linear',keyframes:[0,.5,1].map(time_s=>({time_s,values:[0,0,0]}))})));
 starts.push({id:`${body}-d${duty}-p0${a}${b}${c}`,candidate,align_and_bind_forces:true});
}
assert.equal(starts.length,258);assert.equal(new Set(starts.map(s=>s.id)).size,258);
const batch={robot:base.robot,starts,conic};
const write=(n,v)=>fs.writeFileSync(d+n,JSON.stringify(v,null,2)+'\n',{flag:'wx'});
write('joint-conic-patterns.batch.json',batch);
const selected=['control-fast-original-basis','control-warm-feasible','original-d0.6-p0000','mean-d0.75-p0123'];
write('joint-conic-pattern-pilot.batch.json',{...batch,starts:selected.map(id=>starts.find(s=>s.id===id))});
const legacy=read('joint-start-screen.batch.json');write('joint-conic-pattern-legacy.batch.json',{robot:legacy.robot,starts:legacy.starts.slice(0,1)});
const identity=n=>{let path=d+n,bytes=fs.readFileSync(path);return {path,bytes:bytes.length,sha256:crypto.createHash('sha256').update(bytes).digest('hex')};};
write('joint-conic-pattern-preparation.json',{inputs:['joint-timed-speed.recipe.json','joint-conic-warm-final.recipe.json','joint-start-screen.batch.json'].map(identity),outputs:['joint-conic-patterns.batch.json','joint-conic-pattern-pilot.batch.json','joint-conic-pattern-legacy.batch.json'].map(identity),cases:258,experimental_cases:256,phase_grid:grid,phase_jitter:jitter,stance_fractions:duties,body_references:['unchanged fast reference','constant mean of periodic body controls'],speed_m_s:Math.hypot(...base.candidate.motion.displacement_world_m)/base.candidate.motion.period_s,force_layout:'New cases start with three zero linear nodes, then use shared contact/body-event refinement and timing binding. Controls retain their original force bases.',scope:'Systematic fixed-motion initialization screen at the prior fast target. Physical robot, force bounds, foot centers, swings and tolerances unchanged. Small declared phase offsets avoid coincident events in the current timing representation. A failed start does not exclude its gait family; neither global speed optimality nor runtime feasibility follows.'});
console.log({cases:starts.length,pilot_cases:selected.length,speed_m_s:Math.hypot(...base.candidate.motion.displacement_world_m)/base.candidate.motion.period_s});
