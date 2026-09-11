// Acceptance checks on shared-runtime outputs; no independent physics model.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const root=process.argv[2] ?? path.dirname(new URL(import.meta.url).pathname);
const read=name=>JSON.parse(fs.readFileSync(path.join(root,name),'utf8'));
const coarse=read('pwm-physics-results/results.json').cases;
const fine=read('pwm-convergence-results/results.json').cases;
assert.equal(coarse.length,52);assert.equal(fine.length,6);
for(const c of [...coarse,...fine]) {
  assert.equal(c.error,null,c.name);assert.equal(c.samples,80,c.name);
  for(const k of ['mean_tail_speed_rad_s','peak_sampled_current_a','peak_sampled_speed_rad_s']) assert(Number.isFinite(c[k]),`${c.name}: ${k}`);
}
let symmetry=0;
for(const voltage of [9,12.6]) {
  let last=-Infinity;
  for(const duty of [.025,.05,.1,.2,.3,.4,.5,.6,.7,.8,.9,1]) {
    const positive=coarse.find(c=>c.name===`pwm_v${voltage.toFixed(1)}_d${duty===1?'1.0':duty}`);
    const negative=coarse.find(c=>c.name===`pwm_v${voltage.toFixed(1)}_d-${duty===1?'1.0':duty}`);
    assert(positive&&negative);const speed=positive.mean_tail_speed_rad_s;
    symmetry=Math.max(symmetry,Math.abs(speed+negative.mean_tail_speed_rad_s));
    assert(speed>=last-1e-10,'steady speed must not decrease with drive in these unloaded cases');last=speed;
  }
}
assert(symmetry<1e-8);
const convergence=[];
for(const duty of [.2,1]) {
  const cases=fine.filter(c=>c.name.startsWith(`fine_d${duty===1?'1.0':duty}_`));
  const [a,b]=cases.slice(-2);
  const rel=k=>Math.abs(a[k]-b[k])/Math.max(Math.abs(b[k]),1e-12);
  assert(rel('mean_tail_speed_rad_s')<1e-4);
  assert(rel('peak_sampled_current_a')<.02);
  assert(rel('peak_sampled_speed_rad_s')<.02);
  convergence.push({duty,final_step_s:3.90625e-6,steady_speed_relative_change:rel('mean_tail_speed_rad_s'),sampled_peak_current_relative_change:rel('peak_sampled_current_a'),sampled_peak_speed_relative_change:rel('peak_sampled_speed_rad_s')});
}
const result={passed:true,cases:58,maximum_direction_symmetry_error_rad_s:symmetry,convergence,
  interpretation:'Numerical and software-path validation only. 5 ms sampling does not resolve true startup extrema. Estimated motor/thermal/driver parameters are not calibrated to these nine servos. No concurrent electrical-supply model was validated.'};
fs.writeFileSync(path.join(root,'physics-validation.json'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result,null,2));
