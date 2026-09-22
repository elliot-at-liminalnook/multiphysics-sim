import fs from 'node:fs';
import assert from 'node:assert/strict';
const root = import.meta.dirname;
const read = name => JSON.parse(fs.readFileSync(`${root}/${name}.json`));
const old = read('browser-before.wasm'), current = read('candidate.wasm'), analytic = read('analytic/candidate.wasm'), direct=read('direct/candidate.wasm');
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
const report={
  scope:'Sequential worker measurements in headless Chrome, identical three-second inputs. The old bundle predates the optional matrix lifetime and unpacked matrix kernels; its selected physical recipe is unchanged. The current default and both optional numerical routes are measured separately. No rendering cost is included here. Neither build is a converged/hardware reference.',
  old_browser:stats(old),current_browser:stats(current),analytic_browser:stats(analytic),direct_browser:stats(direct),
  current_speedup:old.wall_s/current.wall_s,analytic_speedup:current.wall_s/analytic.wall_s,direct_speedup:current.wall_s/direct.wall_s,
  old_current_max_abs:differences(old,current),
  native_current_max_abs:differences(read('candidate.native'),current),
  native_analytic_max_abs:differences(read('analytic/candidate.native'),analytic),
  native_direct_max_abs:differences(read('direct/candidate.native'),direct),
  default_direct_max_abs:differences(current,direct),
  default_analytic_max_abs:differences(current,analytic),
  native_worker_transition_comparisons:151,
  initial_worker_learning_scope:'Native transitions include reset at index zero. All 151 complete transition records are compared against worker frame.learning.',
};
assert(Object.values(report.old_current_max_abs).every(value=>value===0), 'preserved/current browser physical and task parity');
for(const comparison of [report.native_current_max_abs,report.native_analytic_max_abs,report.native_direct_max_abs]) {
  assert(Object.values(comparison).every(value=>value<=1e-7), 'native/worker agreement in every saved field');
}
report.exact_browser_parity=true;
report.native_worker_all_saved_fields_within_1e_7=true;
fs.writeFileSync(`${root}/browser-comparison.json`,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify(report,null,2));
