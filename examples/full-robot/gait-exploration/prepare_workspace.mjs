import fs from 'node:fs';
import crypto from 'node:crypto';
const dir='examples/full-robot/gait-exploration';
const source='examples/full-robot/whole-swing/settled-integral-minute.config.json';
const scene='examples/full-robot/whole-swing/settled-integral-candidate.scene.json';
const c=JSON.parse(fs.readFileSync(source));
const obs=c.policy.task_observations;
const rad=d=>d*Math.PI/180;
const samples=[{id:'initial',coordinates:c.initial_coordinates}];
const add=(id,change)=>{let q=[...c.initial_coordinates];change(q);samples.push({id,coordinates:q});};
// These angles are exploration hypotheses. They do not amend CAD hardware limits.
for(let leg=0;leg<4;leg++)for(let hip of [-60,-45,-30,-15,0,15,30,45,60])
 for(let worm of [-150,-75,0,75,150])for(let foot of [-20,-60,-100,-140])
  add(`single-leg${leg}-hip${hip}-worm${worm}-foot${foot}`,q=>{q[3*leg]=rad(hip);q[3*leg+1]+=rad(worm);q[3*leg+2]=rad(foot)});
for(let hip of [-60,-45,-30,-15,0,15,30,45,60])for(let foot of [-20,-60,-100,-140])
 for(let pattern of ['same','alternating','opposing'])add(`coupled-${pattern}-hip${hip}-foot${foot}`,q=>{
  for(let leg=0;leg<4;leg++){q[3*leg]=rad(hip)*(pattern==='same'?1:pattern==='alternating'?(leg%2?-1:1):(leg<2?-1:1));q[3*leg+2]=rad(foot)}
 });
fs.writeFileSync(`${dir}/workspace-markers.json`,JSON.stringify({experiment_id:'coupled-workspace-r1357',coordinate_frame:c.policy.body_feedback.coordinate_frame,expected_cad_sha256:obs.expected_cad_sha256,markers:obs.markers},null,2)+'\n');
fs.writeFileSync(`${dir}/workspace-input.json`,JSON.stringify({independent_coordinates:c.motors.effective.components.map(m=>m.dof),embedding:c.embedding,samples},null,2)+'\n');
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
fs.writeFileSync(`${dir}/workspace-provenance.json`,JSON.stringify({version:1,source,scene,sha256:{[source]:sha(source),[scene]:sha(scene)},samples:samples.length,scope:'Fixed-base sampled kinematics and CAD-authored exclusions; no positive separation margin, continuous clearance, hardware range certification, or loaded stability claim. Every row solves independently from scene seed; initial base vertical offset omitted because relative reach and inter-link overlap are translation invariant.'},null,2)+'\n');
console.log({samples:samples.length});
