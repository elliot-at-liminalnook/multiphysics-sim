// Apply the predeclared numerical screen, using runtime coordinate units.
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [baselinePath,candidatePath,comparisonPath,layoutPath,output]=process.argv.slice(2);
assert(output,'usage: check_precision.mjs baseline candidate comparison layout-capture report');
const read=p=>JSON.parse(readFileSync(p)),hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const a=read(baselinePath),b=read(candidatePath),comparison=read(comparisonPath),layout=read(layoutPath);
assert(a.completed&&b.completed&&layout.completed);
assert.equal(comparison.baseline.sha256,hash(baselinePath));assert.equal(comparison.candidate.sha256,hash(candidatePath));
assert((comparison.numerical_precision||comparison.block_factorization)&&!comparison.timestep_reference);
assert.deepEqual(a.recording.scene,layout.recording.scene);
const coordinates=layout.metadata.frame_coordinates,plan=read('examples/full-robot/browser-precision/plan.json'),limits=plan.comparison_limits;
assert.equal(coordinates.length,a.frames[0].joint_positions.length);
const feet=read('examples/full-robot/foot-markers.json').markers.map(m=>m.link);
const bodyName=a.recording.config.policy.body_feedback.reference_link;
let bodyDifference=0,headingDifference=0,rotationalDifference=0,sliderDifference=0;
const impulse=Object.fromEntries(feet.map(f=>[f,[0,0,0]])),absoluteImpulse=Object.fromEntries(feet.map(f=>[f,0]));
const force=(frame,foot)=>frame.contacts.filter(c=>c.other==null&&frame.poses[c.link].name===foot)
 .reduce((v,c)=>v.map((x,i)=>x+c.force_n[i]),[0,0,0]);
let previous=null;
for(let i=0;i<a.frames.length;i++){
 const x=a.frames[i],y=b.frames[i];assert.equal(x.time_s,y.time_s);
 const p=x.poses.find(p=>p.name===bodyName),q=y.poses.find(p=>p.name===bodyName);
 bodyDifference=Math.max(bodyDifference,Math.hypot(...p.position_m.map((v,j)=>v-q.position_m[j])));
 const delta=Math.atan2(p.rotation[1][0],p.rotation[0][0])-Math.atan2(q.rotation[1][0],q.rotation[0][0]);
 headingDifference=Math.max(headingDifference,Math.abs(Math.atan2(Math.sin(delta),Math.cos(delta))));
 for(const c of coordinates){const d=Math.abs(x.joint_positions[c.index]-y.joint_positions[c.index]);
  if(c.position_unit==='rad')rotationalDifference=Math.max(rotationalDifference,d);
  else {assert.equal(c.position_unit,'m');sliderDifference=Math.max(sliderDifference,d);}
 }
 const differences=Object.fromEntries(feet.map(f=>{const p=force(x,f),q=force(y,f);return [f,p.map((v,j)=>v-q[j])];}));
 if(previous){const h=x.time_s-previous.time_s;for(const f of feet){
  for(let j=0;j<3;j++)impulse[f][j]+=h*(previous.differences[f][j]+differences[f][j])/2;
  absoluteImpulse[f]+=h*(Math.hypot(...previous.differences[f])+Math.hypot(...differences[f]))/2;
 }}
 previous={time_s:x.time_s,differences};
}
const metrics={maximum_foot_difference_m:comparison.metrics.foot_marker_position_m.maximum,
 maximum_body_difference_m:bodyDifference,maximum_joint_difference_rad:rotationalDifference,
 maximum_slider_difference_m:sliderDifference,maximum_heading_difference_rad:headingDifference,
 maximum_per_foot_impulse_difference_ns:Math.max(...Object.values(absoluteImpulse))};
const passed=Object.entries(metrics).every(([name,v])=>Number.isFinite(v)&&v<=limits[name])
 &&comparison.contact_pair_mismatches===0&&comparison.phase_mismatches===0;
const result={version:1,passed,metrics,limits,contact_pair_mismatches:comparison.contact_pair_mismatches,
 phase_mismatches:comparison.phase_mismatches,per_foot_signed_impulse_difference_ns:impulse,
 per_foot_integrated_force_discrepancy_ns:absoluteImpulse,
 sources:[baselinePath,candidatePath,comparisonPath,layoutPath].map(path=>({path,sha256:hash(path)})),
 scope:'Whole 50 Hz sampled trajectory differences against the retained 1e-8 solve, with runtime-declared rotational and translational units. Integrated force discrepancy is trapezoidal integration of force-difference magnitude, so cancellation cannot hide disagreement. Not resolved impact accuracy, hardware calibration or independent walking acceptance.'};
writeFileSync(output,JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({passed,metrics}));
