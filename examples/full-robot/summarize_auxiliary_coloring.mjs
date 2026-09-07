import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/auxiliary-coloring',browser='runs/interactive/auxiliary-coloring';
const artifacts={};
async function bytes(path){const b=await readFile(path);artifacts[path]=createHash('sha256').update(b).digest('hex');return b;}
async function read(path){return JSON.parse(await bytes(path));}
const manifest=await read(directory+'/manifest.json');
const work=d=>{const sums={};for(const s of d.hybrid_solves)for(const[k,v]of Object.entries(s))if(k.startsWith('successful_'))sums[k]=(sums[k]??0)+v;return sums;};
const cases=[];
for(const name of ['base','refined']) {
  const a=await read(`${directory}/${name}.execution.json`),b=await read(manifest.comparisons[name]);
  const c=await read(`${directory}/${name}.comparison.json`);
  await bytes(`${directory}/${name}.config.json`);
  assert(a.completed&&a.error===null&&b.completed&&b.error===null);
  assert.equal(a.implicit.color_auxiliary_jacobian,true);
  assert.equal(a.implicit.condense_auxiliary,true);
  assert.equal(a.scene_options.motor_dynamics??'detailed','detailed');
  const n=a.motor_state_layout.reduce((s,x)=>s+x[1],0),colors=Math.max(...a.motor_state_layout.map(x=>x[1]));
  assert.equal(n,manifest.expected_structural_dimension);assert.equal(colors,manifest.expected_colors);
  const x=work(a),y=work(b);
  assert.equal(x.successful_trial_colored_auxiliary_solves,x.successful_trial_auxiliary_solves);
  assert(x.successful_trial_auxiliary_evaluations<y.successful_trial_auxiliary_evaluations);
  assert(c.exact_frames&&c.exact_terminal_frame&&c.exact_event_schedule);
  assert(isDeepStrictEqual(a.contact_steps,b.contact_steps),'accepted contact trace changed');
  assert(isDeepStrictEqual(a.hybrid_steps,b.hybrid_steps),'hybrid schedule/diagnostics changed');
  assert(Object.values(c.maximum_sampled_differences).every(x=>x===0));
  const closure={};
  for(const f of a.frames)for(const row of f.original_rows){
    const scale=row.unit==='m'?a.embedding.length_scale_m:row.unit==='rad'?a.embedding.angle_scale_rad:1;
    const maximum=closure[row.unit]??={position:0,velocity:0,acceleration:0};
    for(const key of Object.keys(maximum)){
      assert(Number.isFinite(row[key]));maximum[key]=Math.max(maximum[key],Math.abs(row[key]));
      assert(Math.abs(row[key])/scale<=a.embedding.scaled_closure_tolerance);
    }
  }
  cases.push({name,step_s:a.step_s,simulated_s:a.simulated_s,
    inner_dimension:n,inner_colors:colors,exact_sampled_frames:true,exact_terminal_frame:true,
    exact_events_and_subdivisions:true,exact_accepted_contact_trace:true,
    candidate_work:x,reference_work:y,development_candidate_wall_s:a.stepping_wall_s,
    development_reference_wall_s:b.stepping_wall_s,original_closure_maxima:closure,
    profile:a.solver_profile,
  });
}
const benchmark=await read(directory+'/benchmark/manifest.json');
assert.equal(benchmark.runs.length,4);assert.deepEqual(benchmark.runs.map(r=>r.mode),['ordinary','colored','colored','ordinary']);
const original=await read(manifest.comparisons.base);
for(const run of benchmark.runs){
  const d=await read(run.output);assert.equal(artifacts[run.output],run.sha256);
  assert(d.completed&&d.error===null);
  assert(isDeepStrictEqual(d.frames,original.frames),'benchmark sampled trajectory changed');
  assert(isDeepStrictEqual(d.contact_steps,original.contact_steps),'benchmark accepted contact trace changed');
  assert.equal(d.stepping_wall_s,run.stepping_wall_s);
}
const fixture=await read(browser+'/pendulum-browser.json');
const robot=await read(browser+'/robot-browser.json');
const ui=await read(browser+'/viewer-report.json');
assert(fixture.passed&&ui.passed);
const catalog=await read(browser+'/viewer/catalog.json');
if(!robot.passed)assert(!catalog.presets.some(p=>p.id==='robot-point-colored-auxiliary'));
for(const path of [
  'crates/sim-solve/src/coloring.rs','crates/sim-solve/src/lib.rs','crates/sim-solve/tests/coloring.rs',
  'crates/sim-domain-robot/src/articulated/embedding/implicit.rs',
  'crates/sim-domain-robot/src/articulated/embedding/control.rs',
  'crates/sim-domain-robot/src/articulated/embedding/servos.rs',
  'crates/sim-domain-robot/src/articulated/embedding/motor_step.rs',
  'crates/sim-domain-robot/tests/embedded_motor.rs','crates/sim-runtime/tests/embedded_session.rs',
  'examples/full-robot/prepare_auxiliary_coloring.mjs','examples/full-robot/benchmark_auxiliary_coloring.mjs',
  'examples/full-robot/summarize_auxiliary_coloring.mjs','examples/full-robot/auxiliary-coloring-validation.md',
  'examples/interactive/pendulum.condensed.json','.github/workflows/browser.yml','web/tests/viewer.mjs',
  browser+'/native-runner',browser+'/source-manifest.json',browser+'/pendulum.execution.json',
  browser+'/robot-browser-candidate.data.json',
  browser+'/toolchain.json','runs/interactive/robot-lab-auxiliary-coloring-2026-09-07.zip',
  browser+'/viewer/build-manifest.json',browser+'/viewer/sim_web_bg.wasm',directory+'/viewer.config.json',
])await bytes(path);
await writeFile('examples/full-robot/auxiliary-coloring-status.json',JSON.stringify({
  milestone:'structural_auxiliary_coloring',native_numerical_equivalence_to_uncolored_parent:true,
  promoted_default:false,training_model_accepted:false,
  scope:'Exact complete native trajectories relative to uncolored condensation, with fewer derivative probes. Parent condensation differs from the simultaneous reference and remains unaccepted. Browser fixture and full-robot portability outcomes are reported separately; no threshold was loosened. Sequential benchmark excludes other assistant-launched jobs but not host background activity.',
  manifest,cases,benchmark,browser_fixture:fixture,browser_robot:robot,ui,artifacts,
},null,2));
console.log(JSON.stringify({cases:cases.map(c=>({name:c.name,exact:true,component_evaluations:c.candidate_work.successful_trial_auxiliary_evaluations})),benchmark:benchmark.result,browser_fixture:fixture.passed,browser_robot:robot.passed,ui_checks:ui.checks.length}));
