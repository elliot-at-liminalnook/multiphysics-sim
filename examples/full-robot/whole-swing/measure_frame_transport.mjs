import {existsSync,mkdirSync,openSync,closeSync,readFileSync,writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const bundle='runs/interactive/frame-transport/viewer',output='runs/interactive/frame-transport';
const root='examples/full-robot/whole-swing',read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const parityPath=`${output}/parity.json`,parity=read(parityPath);
assert(parity.passed&&parity.replay_exact&&parity.reset_exact&&parity.invalid_encoding_preserved);
assert.equal(parity.frame_encoding,'json');
const cases=[];mkdirSync(output,{recursive:true});
for(const [name,encoding,scenario] of [['object-turn','object','turn-reverse'],['json-turn','json','turn-reverse'],['json-forward','json','sustained-forward']]){
  const report=`${output}/${name}.json`,log=`${output}/${name}.log`;assert(!existsSync(report)&&!existsSync(log));
  const fd=openSync(log,'wx');
  const result=spawnSync(process.execPath,['web/tests/live_performance.mjs',bundle,'tested-portable-tangent-steering',report,scenario],
    {env:{...process.env,DISPLAY_RATE:'0',FRAME_ENCODING:encoding},stdio:['ignore',fd,fd]});closeSync(fd);assert(!result.error,result.error?.message);
  const measurement=existsSync(report)?read(report):null;
  cases.push({name,exit_code:result.status,measurement,sources:[log,...(measurement?[report,report.replace('.json','.recording.json'),report.replace('.json','.timing.json')]:[])].filter(existsSync).map(source)});
  writeFileSync(`${root}/frame-transport-status.json`,JSON.stringify({version:1,complete:cases.length===3,cases,parity,
    sources:[`${root}/FRAME-TRANSPORT-PLAN.md`,parityPath,`${bundle}/build-manifest.json`,'web/tests/live_performance.mjs',import.meta.filename].map(source),
    scope:'Sequential same-bundle object/JSON step-reply cases with automatic display and unchanged 50 Hz Rust physics/control and pacing. Receiving JSON parse time is included in transition latency. All outcomes retained; exact recording preservation and UI checks are separate.'},null,2)+'\n');
  console.log(JSON.stringify({name,completed:measurement?.completed,active:measurement?.performance.active_motion,speed:measurement?.meets_speed_target,latency:measurement?.meets_transition_target}));
}
