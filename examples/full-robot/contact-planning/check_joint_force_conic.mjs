import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import crypto from 'node:crypto';
const d='examples/full-robot/contact-planning/';
const cases=[['warm','joint-ipopt-warm.recipe.json'],['warm-final','joint-conic-warm-final.recipe.json'],['fast','joint-timed-speed.recipe.json']];
const read=p=>JSON.parse(fs.readFileSync(p));
const identity=path=>{let bytes=fs.readFileSync(path);return {path,bytes:bytes.length,sha256:crypto.createHash('sha256').update(bytes).digest('hex')};};
const same=(a,b,message)=>assert(isDeepStrictEqual(JSON.parse(JSON.stringify(a)),JSON.parse(JSON.stringify(b))),message);
const summaries=[];
for(const [name,recipeName] of cases){
 const recipe=read(d+recipeName),r=read(d+`joint-conic-${name}.result.json`);
 assert(r.search.solved);assert.equal(r.search.status,'Solved');
 assert(r.independent_affine_error<1e-8);
 const expected=structuredClone(recipe.candidate);let maxBox=0;
 for(const variable of recipe.variables.filter(v=>v.decision.kind==='force')){const {clock,foot,node,axis}=variable.decision;const f=r.candidate.force_templates[clock][foot].keyframes[node].values[axis];expected.force_templates[clock][foot].keyframes[node].values[axis]=f;maxBox=Math.max(maxBox,variable.bound.lower-f,f-variable.bound.upper);}
 same(expected,r.candidate,'Conic solve changed motion or an unselected force');
 assert.equal(maxBox,r.force_box_violation_n);
 const report=r.report,m=report.motion_report,t=r.search.values.at(-1);
 const maxBalance=Math.max(...m.frames.flatMap(f=>f.wrench_residual.map((x,i)=>Math.abs(x)/(i<3?recipe.robot.force_tolerance_n:recipe.robot.moment_tolerance_nm))));
 assert(maxBalance<=t+1e-6,'Conic balance epigraph disagrees with full CAD report');
 assert.equal(r.search.independently_computed_objective,t);
 summaries.push({name,recipe:identity(d+recipeName),result:identity(d+`joint-conic-${name}.result.json`),status:r.search.status,iterations:r.search.iterations,speed_m_s:m.speed_m_s,minimax_balance:t,measured_maximum_normalized_balance:maxBalance,reported_dual_objective:r.search.reported_dual_objective,primal_dual_gap:r.search.primal_dual_objective_gap,primal_cone_violation:r.search.maximum_primal_cone_violation,dual_cone_violation:r.search.maximum_dual_cone_violation,stationarity_residual:r.search.maximum_stationarity_residual,independent_affine_error:r.independent_affine_error,force_box_violation_n:maxBox,force_error_n:m.maximum_force_error_n,moment_error_nm:m.maximum_moment_error_nm,torque_margin_nm:m.minimum_torque_margin_nm,friction_cone_violation_n:report.maximum_cone_violation_n,sampled_feasible:report.sampled_feasible});
}
const out={summaries,scope:'Independent scalar checks of completed convex fixed-motion force solves. Native numerical optimum is not an interval proof, global speed maximum or runtime validation.'};
fs.writeFileSync(d+'joint-force-conic-verification.json',JSON.stringify(out,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(out));
