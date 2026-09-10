// Verify the generated curves against independently evaluated inverse dynamics.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const d='examples/full-robot/contact-planning',read=p=>JSON.parse(fs.readFileSync(p));
const paths=[d+'/directional-feedforward-reference.result.json',d+'/controller-reference.result.json',d+'/reverse-load-audit.result.json'];
const current=read(paths[0]),old=read(paths[1]),reverse=read(paths[2]).report;
assert.deepEqual(current.trajectory,old.trajectory,'motor reference geometry changed');
assert.deepEqual(current.static_feedforward,old.static_feedforward,'static reference changed');
const n=current.recipe.uniform_samples;let forwardError=0,reverseError=0;
for(let i=0;i<n;i++)for(let j=0;j<current.recipe.independent_coordinates.length;j++) {
 const s=current.static_feedforward.keyframes[i].values[j],v=current.velocity_feedforward.keyframes[i].values[j],a=current.dynamic_feedforward.keyframes[i].values[j];
 forwardError=Math.max(forwardError,Math.abs(v+a-old.dynamic_feedforward.keyframes[i].values[j]));
 const stiffness=current.recipe.actuators[current.recipe.independent_coordinates[j]].stiffness;
 reverseError=Math.max(reverseError,Math.abs((s-v+a)*stiffness-reverse.frames[i].motor_torques_nm[j]));
}
assert(forwardError<1e-12&&reverseError<1e-12);
assert.equal(current.reverse_load_audit.sampled_feasible,false,'known reverse torque violation must remain visible');
fs.writeFileSync(d+'/directional-feedforward-check.json',JSON.stringify({samples:n,maximum_forward_offset_change_rad:forwardError,
 maximum_reverse_load_reconstruction_error_nm:reverseError,unchanged_joint_reference:true,passed:true,
 sources:[...paths,import.meta.filename].map(path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')})),
 scope:'At every original uniform inverse-load sample, new odd/even offsets preserve the forward feedforward and reconstruct the separately evaluated reverse load. Periodic interpolation is linear in these coefficients; finite sampling, intermediate clock rates, acceleration and steering remain approximations.'},null,2)+'\n',{flag:'wx'});
console.log({forwardError,reverseError});
