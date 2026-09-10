import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';

const base = 'examples/full-robot/contact-planning/contact-sequence-';
const read = suffix => JSON.parse(fs.readFileSync(base + suffix));
const single = read('single.result.json'), double = read('double.result.json');
const a = read('single.recipe.json'), b = read('double.recipe.json');
const period = a.motion.period_s;
assert.equal(b.motion.period_s, period * 2);
assert.equal(b.robot.uniform_samples, a.robot.uniform_samples * 2);
assert.deepEqual(b.robot.additional_phases, a.robot.additional_phases.flatMap(p => [p/2, (p+1)/2]));
const restoredRobot = {...b.robot, uniform_samples:a.robot.uniform_samples, additional_phases:a.robot.additional_phases};
assert.deepEqual(restoredRobot, a.robot, 'all physical properties and gates must remain identical');
assert(b.motion.feet.every(f => f.additional_steps.length === 1));
assert.equal(single.sampled_feasible, true);
assert.equal(double.sampled_feasible, true);
assert.equal(double.frames.length, single.frames.length * 2);
assert.equal(single.speed_m_s, double.speed_m_s);

const differences = {};
const compare = (x,y,path) => {
  if (typeof x === 'number') {
    assert(Number.isFinite(x) && Number.isFinite(y), path);
    const error = Math.abs(x-y);
    differences[path] = Math.max(differences[path] ?? 0,error);
    assert(error <= 1e-8, `${path}: ${error}`);
  } else if (Array.isArray(x)) {
    assert(Array.isArray(y) && x.length === y.length,path);
    x.forEach((v,i)=>compare(v,y[i],path+'[]'));
  } else if (x !== null && typeof x === 'object') {
    assert.deepEqual(Object.keys(x),Object.keys(y),path);
    for (const k of Object.keys(x)) compare(x[k],y[k],path+'.'+k);
  } else assert.equal(x,y,path);
};
const visits = new Map(single.frames.map(f=>[f,0]));
for(const frame of double.frames) {
  const match = single.frames.find(old => {
    const delta = (frame.time_s-old.time_s)/period;
    return Math.abs(delta-Math.round(delta)) < 1e-12 &&
      visits.get(old) < 2 && Math.abs(old.residual_weight-2*frame.residual_weight) < 1e-12 &&
      JSON.stringify(frame.clock) === JSON.stringify(old.clock);
  });
  assert(match,'every doubled-cycle frame must match an original phase and clock');
  visits.set(match,visits.get(match)+1);
  const {time_s:ta,residual_weight:wa,...physicalA}=match;
  const {time_s:tb,residual_weight:wb,...physicalB}=frame;
  compare(physicalA,physicalB,'frame');
  // Quadrature weights are defined over the normalized full cycle.
  compare(wa,2*wb,'normalized_interval_weight');
}
assert([...visits.values()].every(n=>n===2));
for(const key of ['maximum_force_error_n','maximum_moment_error_nm','minimum_torque_margin_nm','maximum_penetration_m']) {
  compare(single[key],double[key],key);
}
const evidence = ['single.recipe.json','double.raw.json','double.recipe.json','single.result.json','double.result.json']
  .map(suffix=>({path:base+suffix,sha256:crypto.createHash('sha256').update(fs.readFileSync(base+suffix)).digest('hex')}));
const summary={version:1,passed:true,speed_m_s:single.speed_m_s,frames:[single.frames.length,double.frames.length],
  original_phase_visits:2,absolute_equivalence_tolerance:1e-8,maximum_differences:differences,evidence,
  scope:'Repeated-cycle CAD planning equivalence using the same physical parameters and unchanged feasibility gates. Multi-step point-load allocation passes the sampled planner; this is neither joint per-stance force optimization nor runtime walking or a speed improvement.'};
fs.writeFileSync(base+'equivalence.json',JSON.stringify(summary,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({passed:true,frames:summary.frames,speed_m_s:summary.speed_m_s,maximum_difference:Math.max(...Object.values(differences))}));
