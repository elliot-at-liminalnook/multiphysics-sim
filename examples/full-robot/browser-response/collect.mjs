import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const run='runs/interactive/command-response';
const read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const ui=read(`${run}/ui.json`);assert(ui.passed);
const cases=['probe','turn-reverse'].map(name=>{
 const path=`${run}/${name}.json`,report=read(path),checkPath=`${run}/${name}-check.json`,check=read(checkPath);
 assert(report.completed&&check.passed);
 assert.equal(check.sources[0].sha256,source(path).sha256);
 for(const s of check.sources)assert.equal(source(s.path).sha256,s.sha256);
 return {name,concurrent_training:name==='probe',completed:report.completed,host:report.host,
  performance:report.performance,meets_speed_target:report.meets_speed_target,meets_transition_target:report.meets_transition_target,
  association_check:check,sources:[source(path),source(checkPath)]};
});
const rejection=readFileSync(`${run}/rejected-five-second.log`,'utf8').trim();
assert(rejection.includes('world load boundaries must align with physics steps inside the horizon'));
const result={version:1,ui:{passed:true,checks:ui.checks.length,source:source(`${run}/ui.json`)},cases,
 rejected_probe:{duration_s:5,error:rejection,scope:'Shortening the horizon left the inherited seven-second push outside the episode; rejected before physics advanced.',
  sources:[source(`${run}/rejected-five-second.config.json`),source(`${run}/rejected-five-second.log`)]},
 sources:[source(`${run}/viewer/build-manifest.json`),source('web/tests/live_performance.mjs'),source('web/tests/check_command_response.mjs'),source('web/viewer/viewer.js')],
 scope:'Command-reference association and UI checks. The ten-second functional probe overlapped training and is not performance acceptance. The 24-second turn/reverse measurement ran after training stopped. Render submission is distinct from monitor presentation and physical motion/stopping response; those remain unmeasured.'};
writeFileSync('examples/full-robot/browser-response/status.json',JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({ui:result.ui.passed,cases:cases.map(c=>({name:c.name,commands:c.association_check.commands.length}))}));
