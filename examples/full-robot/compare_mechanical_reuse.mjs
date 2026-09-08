// Whole sampled trajectories, including aggregated contact forces. Report
// numerical differences separately from the independent task acceptance checks.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const [baselinePath,candidatePath,output]=process.argv.slice(2);
const timestepReference=process.argv.includes('--timestep-reference');
const numericalPrecision=process.argv.includes('--numerical-precision');
const blockFactorization=process.argv.includes('--block-factorization');
const guardedBacktracking=process.argv.includes('--guarded-backtracking');
const broydenUpdates=process.argv.includes('--broyden-updates');
const velocitySeed=process.argv.includes('--velocity-seed');
const linearizedProbes=process.argv.includes('--linearized-probes');
assert(baselinePath&&candidatePath&&output,'usage: compare_mechanical_reuse baseline candidate report');
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const a=read(baselinePath),b=read(candidatePath);
assert(a.completed&&b.completed&&!a.error&&!b.error);
assert.deepEqual(a.task,b.task);
const events=c=>(c.recording.input_events??[]).map(e=>({time_ns:Math.round(e.at_step*c.recording.config.step_s*1e9),values:e.values}));
assert.deepEqual(events(a),events(b));
const clean=c=>{c=structuredClone(c);for(const field of ['reuse_step_jacobian','reuse_controller_sample_jacobian','restart_failed_reused_mechanics','cached_mechanical_iteration_limit'])delete c.implicit[field];if(numericalPrecision){delete c.implicit.newton.absolute_tolerance;delete c.implicit.newton.relative_tolerance;}if(blockFactorization)delete c.embedding.block_dependent_factorization;if(guardedBacktracking)delete c.implicit.newton.guarded_backtracking;if(broydenUpdates){delete c.implicit.newton.broyden_updates;delete c.implicit.newton.broyden_negligible_updates;}if(linearizedProbes){delete c.implicit.linearized_jacobian_probes;delete c.implicit.linearized_probe_relative_step;}if(velocitySeed)delete c.implicit.extrapolate_velocity_seed;if(timestepReference){delete c.step_s;delete c.steps;delete c.report_every;}return c;};
assert.deepEqual(clean(a.recording.config),clean(b.recording.config));
assert.deepEqual(a.recording.scene,b.recording.scene);
assert.equal(a.frames.length,b.frames.length);
const metrics={};
function add(name,value,time_s,identity){assert(Number.isFinite(value));const m=metrics[name]??={maximum:0,sum_squared:0,samples:0};m.sum_squared+=value*value;m.samples++;if(value>m.maximum){m.maximum=value;m.worst={time_s,identity};}}
const distance=(a,b)=>Math.hypot(...a.map((v,i)=>v-b[i]));
const markerPath='examples/full-robot/foot-markers.json',markers=read(markerPath);
assert.equal(markers.expected_cad_sha256,a.recording.scene.robot.source.cad_sha256);
const markerPosition=(frame,m)=>{const p=frame.poses.find(p=>p.name===m.link);assert(p);return p.position_m.map((v,i)=>v+p.rotation[i].reduce((s,r,j)=>s+r*m.local_point_m[j],0));};
function contacts(frame){const out=new Map();for(const c of frame.contacts){const key=`${c.link}:${c.other}`,v=out.get(key)??[0,0,0];for(let i=0;i<3;i++)v[i]+=c.force_n[i];out.set(key,v);}return out;}
let contact_pair_mismatches=0,phase_mismatches=0;
for(let i=0;i<a.frames.length;i++){
 const x=a.frames[i],y=b.frames[i],t=x.time_s;assert.equal(t,y.time_s);
 for(const marker of markers.markers)add('foot_marker_position_m',distance(markerPosition(x,marker),markerPosition(y,marker)),t,marker.id);
 assert.deepEqual(x.poses.map(p=>p.name),y.poses.map(p=>p.name));
 for(let j=0;j<x.poses.length;j++){
  const p=x.poses[j],q=y.poses[j];
  add('link_position_m',distance(p.position_m,q.position_m),t,p.name);
  if(p.name===a.recording.config.policy?.body_feedback?.reference_link){
   add('body_position_m',distance(p.position_m,q.position_m),t,p.name);
   const yaw=p=>Math.atan2(p.rotation[1][0],p.rotation[0][0]);
   const error=yaw(p)-yaw(q);
   add('body_heading_rad',Math.abs(Math.atan2(Math.sin(error),Math.cos(error))),t,p.name);
  }
  add('link_velocity_m_s',distance(p.velocity_m_s,q.velocity_m_s),t,p.name);
  add('link_angular_velocity_rad_s',distance(p.angular_velocity_rad_s,q.angular_velocity_rad_s),t,p.name);
  add('rotation_matrix_entry',Math.max(...p.rotation.flat().map((v,k)=>Math.abs(v-q.rotation.flat()[k]))),t,p.name);
 }
 for(const key of ['joint_positions','joint_velocities','servo_targets_rad']) {
  assert.equal(x[key].length,y[key].length);
  for(let j=0;j<x[key].length;j++)add(key,Math.abs(x[key][j]-y[key][j]),t,j);
 }
 const ca=contacts(x),cb=contacts(y),keys=new Set([...ca.keys(),...cb.keys()]);
 if(JSON.stringify([...ca.keys()].sort())!==JSON.stringify([...cb.keys()].sort()))contact_pair_mismatches++;
 for(const key of keys)add('contact_pair_force_n',distance(ca.get(key)??[0,0,0],cb.get(key)??[0,0,0]),t,key);
 if(x.policy?.step_reference?.reference.phase!==y.policy?.step_reference?.reference.phase)phase_mismatches++;
}
for(const m of Object.values(metrics)){m.rms=Math.sqrt(m.sum_squared/m.samples);delete m.sum_squared;}
fs.writeFileSync(output,JSON.stringify({version:1,timestep_reference:timestepReference,numerical_precision:numericalPrecision,block_factorization:blockFactorization,guarded_backtracking:guardedBacktracking,broyden_updates:broydenUpdates,linearized_probes:linearizedProbes,velocity_seed:velocitySeed,baseline:{path:baselinePath,sha256:hash(baselinePath)},candidate:{path:candidatePath,sha256:hash(candidatePath)},markers:{path:markerPath,sha256:hash(markerPath)},frames:a.frames.length,metrics,contact_pair_mismatches,phase_mismatches,
 scope:'Whole trajectories at common 50 Hz reporting times with identical scene, task and controls; configurations may differ only in derivative-reuse flags and, when explicitly requested, physics timestep/report stride, absolute/relative Newton tolerances independent-block factorization, guarded backtracking bounded Broyden updates linearized derivative probes or a guarded temporal velocity seed. Contact forces aggregated by physical link pair. Differences include any altered subdivision sequence. This report measures differences without declaring them acceptable; independent task gates, between-sample contact impulses and hardware accuracy remain separate.'},null,2)+'\n');
console.log(JSON.stringify({metrics,contact_pair_mismatches,phase_mismatches}));
