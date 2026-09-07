import {readFileSync,writeFileSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/browser-precision',run='runs/full-robot/learning/heading-performance';
const read=p=>JSON.parse(readFileSync(p)),hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const plan=read(`${root}/plan.json`),cases=[];
for(const name of ['baseline',...plan.candidates]){
 const path=`${run}/${name}.profile.json`;assert(existsSync(path),path);
 const p=read(path),config=name==='baseline'?plan.baseline_config:`${root}/${name}.config.json`;
 const acceptance=`${run}/${name}-acceptance/summary.json`,screen=`${root}/${name}.screen.json`;
 cases.push({name,config:{path:config,sha256:hash(config)},completed:p.completed,
  profile_sha256:hash(path),profile_wall_s:p.wall_s,buckets:p.buckets,
  accepted_segments:p.accepted_implicit_steps.length,
  fresh_restarts:p.accepted_implicit_steps.filter(s=>s.fresh_restart_reason).length,
  maximum_verified_velocity_residual:Math.max(...p.accepted_implicit_steps.map(s=>s.maximum_scaled_velocity_residual)),
  walking:existsSync(acceptance)?read(acceptance):null,numerical_screen:existsSync(screen)?read(screen):null});
}
const performances=['browser-precision','block-precision','guarded-precision'].flatMap(name=>{
 const p=`runs/interactive/${name}/live-performance.json`;
 if(!existsSync(p))return [];
 const manifest=`runs/interactive/${name}/viewer/build-manifest.json`,record=`runs/interactive/${name}/live-performance.recording.json`;
 return [{name,report:read(p),manifest_sha256:hash(manifest),recording_sha256:hash(record)}];
});
writeFileSync(`${root}/study-status.json`,JSON.stringify({version:1,plan,source_commit:'b21b624',cases,performances,
 scope:'Native profiles include diagnostic overhead and were not all isolated, so work counts and browser timing are reported separately. Buckets nest. Individual accepted-step counters omit rejected cached attempts; global profile counters include them. Neither numerical agreement nor this flat-floor crawl establishes physical calibration or broad interactive acceptance.'},null,2)+'\n');
console.log(JSON.stringify({cases:cases.length,browser_trials:performances.length}));
