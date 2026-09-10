import fs from 'node:fs';const d='examples/full-robot/fast-wasd',read=p=>JSON.parse(fs.readFileSync(`${d}/${p}.json`)),plan=read('profile-plan'),reference=read(`${plan.source}.native`),metrics=read('human-summary');
const body=f=>f.poses.find(p=>p.name.includes('Chassis')).position_m,referenceMetrics=metrics.rows.find(r=>r.name===plan.source),rows=[];
for(const definition of plan.cases){
 const c=read(`${definition.name}.native`),m=metrics.rows.find(r=>r.name===definition.name);
 let maximum=0;for(let i=0;i<Math.min(c.frames.length,reference.frames.length);i++){
  if(Math.abs(c.frames[i].time_s-reference.frames[i].time_s)>1e-9)throw Error('unmatched recording times');
  maximum=Math.max(maximum,Math.hypot(...body(c.frames[i]).map((x,k)=>x-body(reference.frames[i])[k])));
 }
 const speedDifference=m.completed?Math.max(...m.windows.map((w,i)=>Math.abs(w.directed_speed_mm_s-referenceMetrics.windows[i].directed_speed_mm_s)/referenceMetrics.windows[i].directed_speed_mm_s)):null;
 rows.push({...definition,completed:m.completed,maximum_matched_body_difference_m:maximum,maximum_directed_speed_difference_fraction:speedDifference,maximum_slip_ratio:m.maximum_slip_ratio,passes_physical_approximation:m.completed&&maximum<=.003&&speedDifference<=.05,passes_slip:m.maximum_slip_ratio<=.05,native:m.native});
}
fs.writeFileSync(`${d}/profile-summary.json`,JSON.stringify({reference:plan.source,rows,scope:'Matched 20 s physical recordings. Native timings do not establish rendered browser responsiveness.'},null,2)+'\n');console.log(rows);
