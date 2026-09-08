// Run each declared radius sequentially through the production worker.
// Preserve failed parity outcomes; none of these timings includes rendering.
import {readFileSync,writeFileSync,mkdirSync,existsSync,openSync,closeSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [bundle,output='runs/interactive/tangent-radius',reportPath='examples/full-robot/whole-swing/tangent-radius-browser-parity.json']=process.argv.slice(2);
assert(bundle,'usage: check_tangent_radius_browser.mjs bundle [output] [report]');
const root='examples/full-robot/whole-swing',planPath=`${root}/tangent-radius-plan.json`,read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const plan=read(planPath),cases=[];mkdirSync(output,{recursive:true});
const save=()=>writeFileSync(reportPath,JSON.stringify({version:1,complete:cases.length===plan.cases.length,cases,
  sources:[planPath,`${bundle}/build-manifest.json`,'web/tests/environment.mjs',import.meta.filename].map(source),
  scope:'Sequential native/WASM portability and exact replay/reset at all declared probe radii. Worker round-trip timings exclude rendering; rendered acceptance requires a separate run.'},null,2)+'\n');
for(const c of plan.cases) {
  const native=`${c.config.replace(/\.config\.json$/,'.native.json')}`,report=`${output}/${c.name}-parity.json`,log=`${output}/${c.name}-parity.log`;
  assert(!existsSync(report)&&!existsSync(log),'refusing to overwrite a measured case');
  const fd=openSync(log,'wx');
  const result=spawnSync(process.execPath,['web/tests/environment.mjs',bundle,'tested-tangent-steering-short',native,report,c.config],{stdio:['ignore',fd,fd]});
  closeSync(fd);assert(!result.error,result.error?.message);
  const measurement=existsSync(report)?read(report):null;
  cases.push({name:c.name,probe_relative_step:c.probe_relative_step,exit_code:result.status,measurement,
    sources:[c.scene,c.config,c.task,c.actions,native,log,...(measurement?[report]:[])].map(source)});
  save();console.log(JSON.stringify({name:c.name,passed:measurement?.passed??false,p95_s:measurement?.performance.transition_p95_s}));
}
