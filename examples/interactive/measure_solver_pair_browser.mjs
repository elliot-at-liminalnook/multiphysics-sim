// Same-binary rendered comparison of an explicitly packaged solver pair.
import {existsSync,mkdirSync,openSync,closeSync,readFileSync,writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [studyPath,bundle,output,referencePreset,enabledPreset,parityPath,statusPath]=process.argv.slice(2);
assert(statusPath,'usage: measure_solver_pair_browser study bundle output reference-preset enabled-preset parity status');
const read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const study=read(studyPath),parity=read(parityPath);
assert(parity.passed&&parity.replay_exact&&parity.reset_exact);
assert.equal(parity.frame_encoding,'json');
const cases=[];mkdirSync(output,{recursive:true});
const manifest=source(`${bundle}/build-manifest.json`);
for(const [name,preset,scenario] of [['reference-turn',referencePreset,'turn-reverse'],['enabled-turn',enabledPreset,'turn-reverse'],['enabled-forward',enabledPreset,'sustained-forward']]){
  assert.deepEqual(source(manifest.path),manifest,'bundle changed during measurement');
  const report=`${output}/${name}.json`,log=`${output}/${name}.log`;assert(!existsSync(report)&&!existsSync(log));
  const fd=openSync(log,'wx');
  const result=spawnSync(process.execPath,['web/tests/live_performance.mjs',bundle,preset,report,scenario],
    {env:{...process.env,DISPLAY_RATE:'0',FRAME_ENCODING:'json'},stdio:['ignore',fd,fd]});closeSync(fd);assert(!result.error,result.error?.message);
  const measurement=existsSync(report)?read(report):null;
  cases.push({name,preset,scenario,exit_code:result.status,measurement,sources:[log,...(measurement?[report,report.replace('.json','.recording.json'),report.replace('.json','.timing.json')]:[])].filter(existsSync).map(source)});
  assert.deepEqual(source(manifest.path),manifest);
  writeFileSync(statusPath,JSON.stringify({version:1,complete:cases.length===3,solver_option:study.boolean_solver_option,cases,parity,
    sources:[studyPath,parityPath,'web/tests/live_performance.mjs',import.meta.filename].map(source).concat(manifest),
    scope:'Sequential rendered off/on steering and enabled forward/stop with the same WASM, JSON frame transport, automatic display, unchanged 50 Hz control and original realtime gates. All outcomes retained; native recording identity and UI checks are separate.'},null,2)+'\n');
  console.log(JSON.stringify({name,completed:measurement?.completed,active:measurement?.performance.active_motion,speed:measurement?.meets_speed_target,latency:measurement?.meets_transition_target}));
}
