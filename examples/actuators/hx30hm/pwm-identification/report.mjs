import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const root=path.dirname(new URL(import.meta.url).pathname);
const data=JSON.parse(fs.readFileSync(path.join(root,'results.json')));
const observations=JSON.parse(fs.readFileSync(path.join(root,'observations.json')));
const hardware=path.resolve(root,'../hardware/2026-09-11-nine-servos');
assert.equal(data.models.length,9);assert.equal(observations.length,63);
const measuredRuns=new Map();
for(const t of observations) {
  if(!measuredRuns.has(t.run)) measuredRuns.set(t.run,JSON.parse(fs.readFileSync(path.join(hardware,t.run,'run.json'))));
  const run=measuredRuns.get(t.run);
  const pulses=run.result.completed_pulses.filter(p=>p.id===t.id).sort((a,b)=>a.stage-b.stage);
  const pulse=pulses.find(p=>p.stage===t.stage);
  const previous=pulses.find(p=>p.stage===t.stage-1);
  const pre=JSON.parse(fs.readFileSync(path.join(hardware,t.run,'preflight.json')));
  const expected=pulse.position_raw-(previous?.position_raw??pre.servos[t.id].telemetry.position_raw);
  const observed=t.observation.samples.at(-1)[1]*4096/(2*Math.PI);
  assert(Math.abs(expected-observed)<1e-8,`${t.run}/${t.id}/${t.stage}: baseline mismatch`);
  assert.equal(t.observation.input,run.plan.drive_counts[t.stage]/1000);
  assert(t.observation.duration_s>0&&t.observation.duration_s<.3);
}
const acquisitionLimits=[];
for(const name of ['pwm-pilot-id12-v1','pwm-individual-ids4-11-v1']) {
 const lines=fs.readFileSync(path.join(hardware,name,'pwm.csv'),'utf8').trim().split('\n');
 const keys=lines.shift().split(',');let rawGcd=0,maxWindow=0;
 const gcd=(a,b)=>{while(b){const t=b;b=a%b;a=t;}return a;};
 for(const line of lines) {const row=Object.fromEntries(line.split(',').map((v,i)=>[keys[i],v]));
  rawGcd=gcd(rawGcd,Number(row.speed_raw)&32767);
  maxWindow=Math.max(maxWindow,Number(row.completion_s)-Number(row.request_s));
 }
 acquisitionLimits.push({run:name,speed_raw_gcd_counts_per_s:rawGcd,speed_increment_deg_s:rawGcd*360/4096,maximum_host_read_window_ms:maxWindow*1000});
}
const summary=[];
const failures=[];
let heldout=0,passed=0;
const lines=['# Measured PWM response: preliminary identification','',
  'Offline analysis of 63 recorded pulses from all nine servos. No hardware was accessed. Training uses each servo’s positive 25, 100 and 200/1000 pilot pulses; the 50 and 150/1000 pulses and independent short ±100/1000 pulses are held out. Fits use encoder displacement and the logged command/read windows, not the coarse internal speed register.','',
  'The shared Rust fitter estimates an empirical input/output response: deadband, gain, drive/release lags, and combined delay. It does not identify motor resistance, torque constant, inertia, friction, or sensor latency separately. Values below are provisional and valid only over the observed conditions and drive range.','',
  '| ID | Fitted deadband % | Drive / release lag ms | Combined delay ms | Worst held-out RMSE, counts | Held-out passes | Solver status |',
  '|---|---:|---:|---:|---:|---:|---|'];
for(const row of data.models) {
 const e=row.evaluations.filter(e=>e.split!=='train');
 const acceptable=e.filter(e=>e.metrics.rmse_encoder_counts<=3&&Math.abs(e.metrics.final_displacement_error_counts)<=5);
 heldout+=e.length;passed+=acceptable.length;
 const worst=Math.max(...e.map(e=>e.metrics.rmse_encoder_counts));
 const status=row.fit.termination;
 const m=row.model;
 lines.push(`| ${row.id} | ${(100*m.deadband).toFixed(2)} | ${(1000*m.drive_tau_s).toFixed(1)} / ${(1000*m.release_tau_s).toFixed(1)} | ${(1000*m.delay_s).toFixed(1)} | ${worst.toFixed(2)} | ${acceptable.length}/${e.length} | ${status}${row.active_parameter_bounds_indices.length?' (bound active)':''} |`);
 for(const x of e) if(!acceptable.includes(x))failures.push({id:row.id,...x});
 summary.push({id:row.id,held_out_pulses:e.length,held_out_passes:acceptable.length,all_held_out_within_tolerance:acceptable.length===e.length,optimizer_stationary:status==='stationary',active_parameter_bounds_indices:row.active_parameter_bounds_indices});
}
assert.equal(heldout,36);
lines.push('',`**${passed}/${heldout} held-out pulses pass** the predeclared limits: encoder RMSE ≤3 counts (0.264°) and final displacement error ≤5 counts (0.439°). This is a predictive acceptance test, not a confidence interval or physical calibration certificate.`,'',
  '## What the failures tell us','',
  '- **ID 8:** the simple model predicts motion at 50/1000 where this particular test recorded none. Its held-out 150/1000 response also misses tolerance. One fixed deadband and gain do not explain all its low-drive behavior. Breakaway friction, control behavior, starting angle, and experimental variation remain competing explanations.',
  '- **ID 9:** the short reverse pulse travels less than the symmetric model predicts. A single reverse trial is insufficient to attribute this uniquely to directional friction or motor asymmetry; repeat it at matched angle, voltage, temperature and load.',
  '- **ID 10:** the initial fit hit the zero-delay bound and the iteration budget. An active-bound refinement followed by reopening all original bounds reached stationarity, with essentially unchanged predictions. The zero-delay boundary still prevents interpreting this as measured physical latency.','',
  '## Acquisition and interpretation limits','',
  'All nonzero pilot speed-register readings are multiples of 50 counts/s (about 4.4°/s), while position resolution is one count (0.088°). Some host read windows span 49–59 ms. Residual scales include one encoder count plus observed speed × half that window; this is a descriptive weighting, not a calibrated stochastic noise model. Request/complete timestamp sensitivity fits are saved in results.json. Unknown internal sample age is outside those timing brackets.',
  'On-drive empirical lags are roughly 14–33 ms and release lags roughly 9–15 ms in these unloaded records. Their unequal values support testing zero-drive braking/coasting explicitly. They must not be copied into CAD as physical inertia or electrical constants. Voltage and temperature differ between runs; fitted servo differences are not isolated manufacturing tolerances.',
  'No full-duty speed prediction, torque estimate, confidence interval, thermal fit, or loaded dynamics claim follows from these 2.5–20% drive pulses. No fitted values have been promoted into CAD or default simulation parameters.','',
  '## Next measurements selected by these results','',
  '1. Repeated low-drive onset tests, especially ID 8: 25–100/1000 in fine increments, both directions, multiple starting angles. Record no-motion outcomes rather than forcing a linear curve through them.',
  '2. Continuous ascending/descending drive segments to distinguish breakaway from running friction. The current stationary-start pulse sweep alone cannot measure that hysteresis.',
  '3. Matched repeated +100/−100 short pulses for ID 9 and the others; vary duration at fixed drive to separate delay from rise/release dynamics.',
  '4. Zero-drive versus torque-off versus controlled reversal, initially at small drive, to identify braking/coast behavior before higher-speed reversals.',
  '5. Supply/temperature-conditioned full-drive runs, known loads, and shared-supply concurrent runs. Reserve whole later runs for validation, not just random adjacent samples.','',
  '## Reproduce','',
  '```sh',
  'cargo test --release -p sim-solve pulse_response',
  'cargo run --release -p sim-runtime --example identify_hx_pwm -- \\',
  '  examples/actuators/hx30hm/hardware/2026-09-11-nine-servos NEW_OUTPUT',
  '```','',
  'Inputs are hash-referenced in results.json; observation extraction is independently checked against the measured final-position records by this report generator. The original fit source and two analytic/recovery test results are retained alongside the report.');
fs.writeFileSync(path.join(root,'README.md'),lines.join('\n')+'\n');
fs.writeFileSync(path.join(root,'validation.json'),JSON.stringify({schema_version:1,source_pulses_verified:63,training_pulses:27,held_out_pulses:heldout,held_out_passes:passed,criteria:{maximum_rmse_counts:3,maximum_absolute_final_error_counts:5},per_servo:summary,failures,physical_calibration_accepted:false,acquisition_limits:acquisitionLimits},null,2)+'\n');
console.log(`${passed}/${heldout} held-out pulses passed; ${failures.length} failed. Physical calibration remains unproven.`);
