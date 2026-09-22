import fs from 'node:fs';
import assert from 'node:assert/strict';
const root=import.meta.dirname,read=name=>JSON.parse(fs.readFileSync(`${root}/${name}.json`));
function stats(run) {
  assert(!run.error && run.frames.at(-1).time_s === 3);
  const times = [...run.transition_wall_s].sort((a,b) => a-b);
  return {wall_s:run.wall_s, simulation_per_wall:3/run.wall_s,
    policy_step_p95_s:times[Math.ceil(times.length*0.95)-1]};
}
function differences(a,b) {
  assert.equal(a.frames.length,b.frames.length);
  const max={};
  function walk(x,y,field) {
    if (typeof x === 'number') {
      assert(Number.isFinite(x) && Number.isFinite(y));
      max[field]=Math.max(max[field]||0,Math.abs(x-y));
    } else if (Array.isArray(x)) {
      assert.equal(x.length,y.length);x.forEach((v,i)=>walk(v,y[i],field));
    } else if (x && typeof x === 'object') {
      assert.deepEqual(Object.keys(x).sort(),Object.keys(y).sort());
      for (const key of Object.keys(x)) walk(x[key],y[key],field);
    } else assert.equal(x,y);
  }
  for (let i=0;i<a.frames.length;i++) {
    assert.equal(a.frames[i].time_s,b.frames[i].time_s);
    // The native harness stores transitions beside frames; the worker attaches
    // the same transition as frame.learning. Compare their full contents below.
    const nativeTransitions=Array.isArray(a.transitions);
    assert.deepEqual(Object.keys(a.frames[i]).sort(),Object.keys(b.frames[i])
      .filter(key=>!(nativeTransitions && key==='learning')).sort());
    for (const key of Object.keys(a.frames[i])) {
      if(key!=='stepping_wall_s')walk(a.frames[i][key],b.frames[i][key],key);
    }
    if(nativeTransitions) {
      assert.equal(a.transitions.length,a.frames.length);
      walk(a.transitions[i],b.frames[i].learning,'learning');
    }
  }
  return max;
}
const current=read('candidate.wasm'), old=read('browser-before.wasm');
const report={scope:'Sequential native/worker three-second replay; all saved fields and complete transitions compared. Timings are single runs, not repeated estimates or calibration evidence.', old_browser:stats(old),default:stats(current), default_parity:differences(old,current),runs:{}};
assert(Object.values(report.default_parity).every(value=>value===0));
for(const folder of ['',...Object.keys(read('protocol').variants)]) {
 const prefix=folder?`${folder}/candidate`:'candidate',worker=read(`${prefix}.wasm`),native=read(`${prefix}.native`);
 const parity=differences(native,worker);assert(Object.values(parity).every(value=>value<=1e-7),'native/worker saved field agreement');
 report.runs[folder||'default']={...stats(worker),speedup:current.wall_s/worker.wall_s,native_worker_max_abs:parity,default_worker_max_abs:differences(current,worker)};
 console.log(folder||'default',JSON.stringify({wall_s:worker.wall_s,speedup:current.wall_s/worker.wall_s}));
}
fs.writeFileSync(`${root}/browser-comparison.json`,JSON.stringify(report,null,2)+'\n');
