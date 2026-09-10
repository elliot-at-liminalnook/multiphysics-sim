// Dense observation of the same recorded physics, with endpoint identity checks.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import {spawnSync} from 'node:child_process';
import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
const name=process.argv[2]??'smooth-dynamic-v150-scale1-human-fine';
const spec=process.argv[3]?read(process.argv[3]):{windows:[['forward',2,2.4],['turn',6.5,7]],require_full_coverage:false};
const samplePeriod=spec.sample_period_s??.00125;
assert(Number.isFinite(samplePeriod)&&samplePeriod>0&&samplePeriod<=.0025,'dense sample period must be positive and at most 2.5 ms');
assert(Math.abs(.02/samplePeriod-Math.round(.02/samplePeriod))<1e-8,'dense period must divide original 20 ms reporting');
assert(Array.isArray(spec.windows)&&spec.windows.length);
assert(spec.windows.every(w=>w.length===3&&/^[a-z0-9-]+$/.test(w[0])&&Number.isFinite(w[1])&&Number.isFinite(w[2])&&w[2]>w[1]));
assert.equal(new Set(spec.windows.map(w=>w[0])).size,spec.windows.length);
const def=read(`${d}/validation-cases.json`).rows.find(r=>r.name===name);assert(def,'known case required');
const capture=read(`${def.prefix}.native.json`);assert(capture.completed&&!capture.error,'completed original capture required');
assert(Math.abs(samplePeriod/capture.recording.config.step_s-Math.round(samplePeriod/capture.recording.config.step_s))<1e-8,'dense period must align with unchanged physics steps');
const recordPath=`${def.prefix}.dense.recording.json`;
if(!fs.existsSync(recordPath))fs.writeFileSync(recordPath,JSON.stringify(capture.recording)+'\n',{flag:'wx'});
assert.deepEqual(read(recordPath),capture.recording,'source recording changed');
const original=new Map(capture.frames.map(f=>[f.completed_steps,f]));
const originalRequirements=read(`${def.prefix}.planned-lift-requirements.json`);
const rows=[],coveredRequirements=new Set();
function execute(output,error,args) {
  if(fs.existsSync(output))return;
  const out=fs.openSync(output,'wx'),err=fs.openSync(error,'wx');
  const result=spawnSync(`${bin}/${args[0]}`,args.slice(1),{stdio:['ignore',out,err],timeout:Math.max(180000,def.duration_s*10000)});fs.closeSync(out);fs.closeSync(err);
  assert.equal(result.status,0,`command failed: ${args[0]} (${error})`);
}
function mismatch(a,b,path='') {
  if(isDeepStrictEqual(a,b))return null;
  if(a&&b&&typeof a==='object'&&typeof b==='object') {
    for(const key of new Set([...Object.keys(a),...Object.keys(b)])){const found=mismatch(a[key],b[key],`${path}.${key}`);if(found)return found;}
  }
  return {path,original:a,replayed:b};
}
const physical=frame=>{const {stepping_wall_s,policy_inputs,...rest}=frame;return rest;};
for(const [label,start,end] of spec.windows) {
  const prefix=`${def.prefix}.dense-${label}`, windowPath=`${prefix}.window.json`;
  execute(windowPath,`${prefix}.error.txt`,['capture_embedded_window','--replay',recordPath,String(start),String(end),String(samplePeriod)]);
  const window=read(windowPath);
  assert(window.window_complete&&!window.error&&!window.full_motion_complete,'bounded successful prefix required');
  assert.deepEqual(window.window_s,[start,end]);assert.equal(window.sample_period_s,samplePeriod);
  assert.deepEqual(window.recording.scene,capture.recording.scene);assert.deepEqual(window.config,capture.recording.config);assert.equal(window.seed,capture.recording.seed);
  let compared=0;
  for(const frame of window.frames) {
    const old=original.get(frame.completed_steps);if(!old)continue;
    const difference=mismatch(physical(old),physical(frame));assert.equal(difference,null,JSON.stringify(difference));compared++;
  }
  assert.equal(compared,Math.round(end/.02)-Math.ceil(start/.02)+1,'all original endpoints in window must be checked');
  const requirements=originalRequirements.filter(r=>r.start_s>=start&&r.end_s<=end).map(r=>({...r,maximum_sample_gap_s:samplePeriod+.000001}));
  for(const r of originalRequirements.filter(r=>r.start_s>=start&&r.end_s<=end)) {
    const key=JSON.stringify(r);assert(!coveredRequirements.has(key),'dense windows must not count a lift twice');coveredRequirements.add(key);
  }
  assert(requirements.length,'window must contain original planned lifts');
  const reqPath=`${prefix}.requirements.json`;
  if(!fs.existsSync(reqPath))fs.writeFileSync(reqPath,JSON.stringify(requirements,null,2)+'\n',{flag:'wx'});
  assert.deepEqual(read(reqPath),requirements);
  const auditPath=`${prefix}.geometry.json`;
  execute(auditPath,`${prefix}.geometry-error.txt`,['evaluate_lift',`${def.prefix}.scene.json`,windowPath,reqPath,'--simulation-time']);
  const audit=read(auditPath);
  rows.push({label,window_s:[start,end],period_s:samplePeriod,frames:window.frames.length,original_endpoints_exact:compared,
    window_capture_sha256:hash(windowPath),audit_sha256:hash(auditPath),reports:audit.reports,inter_link_geometry_audit:audit.inter_link_geometry_audit});
}
if(spec.require_full_coverage)assert.equal(coveredRequirements.size,originalRequirements.length,'every original planned lift must be covered exactly once');
const result={source:name,source_capture_sha256:hash(`${def.prefix}.native.json`),spec,planned_lifts_covered:coveredRequirements.size,original_planned_lifts:originalRequirements.length,rows,
  scope:`Bounded replay of unchanged config/seed/recorded inputs with ${samplePeriod*1000} ms endpoint observations. Every common 20 ms physical/policy endpoint must match exactly, excluding wall time and host-boundary policy_inputs (next action is prequeued in replay). Original lift intervals, clearance, unloading and support requirements retained; only the allowed sample gap is tightened. Optional full coverage covers the original declared steady-window lift requirements, not every instant of the full episode or between-sample geometry.`};
fs.writeFileSync(`${d}/${name}.dense-summary.json`,JSON.stringify(result,null,2)+'\n');console.log(rows.map(r=>({label:r.label,endpoints:r.original_endpoints_exact,reports:r.reports.map(v=>({link:v.requirements.swing_link,report:v.report}))})));
