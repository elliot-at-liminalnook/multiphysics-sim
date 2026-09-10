import fs from 'node:fs';
const dir='examples/full-robot/gait-exploration';
const r=JSON.parse(fs.readFileSync(`${dir}/workspace-results.json`));
const ok=x=>!x.error&&!x.authored_limit_violations.length&&!x.sampled_penetrations.length;
const counts=rows=>({samples:rows.length,solver_errors:rows.filter(x=>x.error).length,authored_limit_violations:rows.filter(x=>x.authored_limit_violations.length).length,sampled_overlap:rows.filter(x=>x.sampled_penetrations.length).length,passes_sampled_pose_screen:rows.filter(ok).length});
const errors={}; const pairs={};
for(let x of r.rows){if(x.error)errors[x.error]=(errors[x.error]??0)+1;for(let p of x.sampled_penetrations){let k=[r.link_names[p.link],r.link_names[p.other]].sort().join(' / ');pairs[k]??={samples:0,maximum_penetration_m:0};pairs[k].samples++;pairs[k].maximum_penetration_m=Math.max(pairs[k].maximum_penetration_m,p.penetration_m)}}
const initial=r.rows.find(x=>x.id==='initial');
const legs=initial.markers.map((m,i)=>{
 let rows=r.rows.filter(x=>x.id.startsWith(`single-leg${i}-`)&&ok(x));
 let initialJ=m.jacobian.map(row=>row.slice(3*i,3*i+3));
 return {id:m.id,initial_position_world_m:m.position_world_m,initial_jacobian_m_per_rad:initialJ,
  sampled_valid_foot_bounds_world_m:[0,1,2].map(k=>[Math.min(...rows.map(x=>x.markers[i].position_world_m[k])),Math.max(...rows.map(x=>x.markers[i].position_world_m[k]))]),
  by_hip_deg:Object.fromEntries([-60,-45,-30,-15,0,15,30,45,60].map(h=>[h,counts(r.rows.filter(x=>x.id.startsWith(`single-leg${i}-hip${h}-`)))]))};
});
const summary={version:1,scope:'Sampled fixed-base assembly screen only; no trajectory clearance or loaded gait qualification.',inspection_wall_s:r.inspection_wall_s,poses_per_s:r.rows.length/r.inspection_wall_s,counts:counts(r.rows),initial:counts([initial]),legs,coupled:Object.fromEntries(['same','alternating','opposing'].map(p=>[p,counts(r.rows.filter(x=>x.id.startsWith(`coupled-${p}-`)))])),errors,collision_pairs:Object.entries(pairs).sort((a,b)=>b[1].samples-a[1].samples)};
fs.writeFileSync(`${dir}/workspace-summary.json`,JSON.stringify(summary,null,2)+'\n');
console.log(JSON.stringify({counts:summary.counts,inspection_wall_s:summary.inspection_wall_s,initial:summary.initial,legs:summary.legs,coupled:summary.coupled,errors:Object.entries(errors).slice(0,5),collision_pairs:summary.collision_pairs.slice(0,5)},null,2));
