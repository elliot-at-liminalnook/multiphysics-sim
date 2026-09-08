import {existsSync,mkdirSync,openSync,closeSync,readFileSync,writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const bundle='runs/interactive/display-cadence/viewer',output='runs/interactive/display-cadence';
const root='examples/full-robot/whole-swing',source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const cases=[];mkdirSync(output,{recursive:true});
for(const [name,display,scenario] of [['auto-turn','0','turn-reverse'],['capped-turn','30','turn-reverse'],['capped-forward','30','sustained-forward']]){
  const report=`${output}/${name}.json`,log=`${output}/${name}.log`;assert(!existsSync(report)&&!existsSync(log));
  const fd=openSync(log,'wx');
  const result=spawnSync(process.execPath,['web/tests/live_performance.mjs',bundle,'tested-portable-tangent-steering',report,scenario],
    {env:{...process.env,DISPLAY_RATE:display},stdio:['ignore',fd,fd]});closeSync(fd);assert(!result.error,result.error?.message);
  const measurement=existsSync(report)?JSON.parse(readFileSync(report)):null;
  cases.push({name,exit_code:result.status,measurement,sources:[log,...(measurement?[report,report.replace('.json','.recording.json'),report.replace('.json','.timing.json')]:[])].filter(existsSync).map(source)});
  writeFileSync(`${root}/display-cadence-status.json`,JSON.stringify({version:1,complete:cases.length===3,cases,
    sources:[`${root}/DISPLAY-CADENCE-PLAN.md`,`${bundle}/build-manifest.json`,`${bundle}/viewer.js`,`${bundle}/index.html`,`${bundle}/viewer.css`,'web/tests/live_performance.mjs',import.meta.filename].map(source),
    scope:'Sequential same-bundle rendered cases, automatic and explicit 30 fps display. Physics/control and simulation pacing unchanged. All timing outcomes are retained; physical recipe and input preservation plus UI checks are separate.'},null,2)+'\n');
  console.log(JSON.stringify({name,completed:measurement?.completed,active:measurement?.performance.active_motion,speed:measurement?.meets_speed_target,latency:measurement?.meets_transition_target}));
}
