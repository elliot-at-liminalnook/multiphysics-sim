// Exercise the real CLI against an independently captured complete fixture.
import {spawnSync} from 'node:child_process';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const [runner,referenceRunner]=process.argv.slice(2);
assert(runner&&referenceRunner,'provide capture_embedded_window and integrate_embedding executables');
const scene='examples/interactive/pendulum.scene.json',config='examples/interactive/pendulum.embedded.json';
function run(program,args){const r=spawnSync(program,args,{encoding:'utf8',maxBuffer:16*1024*1024});assert.ifError(r.error);return r;}
const full=run(referenceRunner,[scene,config]);assert.equal(full.status,0,full.stderr);
const reference=JSON.parse(full.stdout);assert(reference.completed&&reference.error===null);
const sample=run(runner,[scene,config,'.004','.016','.004']);assert.equal(sample.status,0,sample.stderr);
const report=JSON.parse(sample.stdout);assert(report.window_complete&&!report.full_motion_complete&&report.error===null);assert.equal(report.frames.length,4);
for(const frame of report.frames){const expected=reference.frames.find(f=>Math.abs(f.time_s-frame.time_s)<1e-12);assert(expected);assert(isDeepStrictEqual(frame,expected),'capture changed the fixture');}
for(const times of [['.0041','.016','.004'],['.004','.016','0'],['.016','.004','.004'],['0','.024','.004'],['NaN','.016','.004']]){
 const bad=run(runner,[scene,config,...times]);assert.equal(bad.status,2);assert.equal(bad.stdout,'');assert(bad.stderr.length>0);
}
console.log(JSON.stringify({passed:true,identical_frames:4,invalid_requests_rejected:5,partial_window_labeled:true}));
