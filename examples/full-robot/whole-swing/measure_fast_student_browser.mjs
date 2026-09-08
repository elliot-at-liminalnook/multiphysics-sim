import {existsSync,openSync,closeSync,readFileSync,writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',run='runs/interactive/fast-distilled',bundle=`${run}/viewer`;
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const parityPath=`${root}/fast-student-browser-parity.json`,parity=read(parityPath);
assert(parity.passed&&parity.replay_exact&&parity.reset_exact);
const manifest=source(`${bundle}/build-manifest.json`),cases=[];
for(const [name,preset,scenario] of [['previous-turn','tested-exact-base-steering','turn-reverse'],['student-turn','tested-fast-distilled-steering','turn-reverse'],['student-forward','tested-fast-distilled-steering','sustained-forward']]){
  const report=`${run}/${name}.json`,log=`${run}/${name}.log`;assert(!existsSync(report)&&!existsSync(log));
  assert.deepEqual(source(manifest.path),manifest);
  const fd=openSync(log,'wx'),result=spawnSync(process.execPath,['web/tests/live_performance.mjs',bundle,preset,report,scenario],{env:{...process.env,DISPLAY_RATE:'0',FRAME_ENCODING:'json'},stdio:['ignore',fd,fd]});closeSync(fd);assert(!result.error,result.error?.message);
  const measurement=existsSync(report)?read(report):null;
  cases.push({name,preset,scenario,exit_code:result.status,measurement,sources:[log,...(measurement?[report,report.replace('.json','.recording.json'),report.replace('.json','.timing.json')]:[])].filter(existsSync).map(source)});
  assert.deepEqual(source(manifest.path),manifest);
  writeFileSync(`${root}/fast-student-browser-status.json`,JSON.stringify({version:1,complete:cases.length===3,cases,parity,sources:[parityPath,`${root}/FAST-STUDENT-FIDELITY-PLAN.md`,'web/tests/live_performance.mjs',import.meta.filename].map(source).concat(manifest),
    scope:'Sequential same-WASM rendered previous/new student steering and new forward/stop, automatic display and JSON transport. Fixed 50 Hz interface, >=1 active/overall pace and <=20 ms p95 gates. All results retained; physical acceptance, exact recordings and UI verification remain separate.'},null,2)+'\n');
  console.log({name,completed:measurement?.completed,active:measurement?.performance.active_motion,speed:measurement?.meets_speed_target,latency:measurement?.meets_transition_target});
}
