import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {gzipSync,gunzipSync} from 'node:zlib';
import {execFileSync} from 'node:child_process';

const root='examples/full-robot/contact-planning/';
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=b=>crypto.createHash('sha256').update(b).digest('hex');
const evidence=p=>({path:p,bytes:fs.statSync(p).size,sha256:hash(fs.readFileSync(p))});
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v,null,2)+'\n',{flag:'wx'});
const old='runs/contact-order-neighbors',matched='runs/contact-order-neighbors-matched';
const q=read(old+'/queue.json'),m=read(matched+'/queue.json');
assert.equal(q.items.length,7);assert.equal(m.items.length,8);
assert.equal(q.unprepared_edges.length,2);
for(const row of q.items) {
  assert.deepEqual(read(matched+'/'+row.id+'.recipe.json'),read(row.recipe));
  assert.deepEqual(read(matched+'/'+row.id+'.conic.json'),read(row.conic));
}
const source=read(root+'contact-order-source.recipe.json');
const refined=read(matched+'/control-refined.recipe.json');
assert.deepEqual(refined.robot,source.robot);
assert.deepEqual(refined.candidate.motion,source.candidate.motion);
assert.deepEqual(refined.variables.filter(v=>v.decision.kind==='motion'),
  source.variables.filter(v=>v.decision.kind==='motion'));
assert.equal(read(matched+'/control-refined.conic.json').report.sampled_feasible,true);
assert(fs.readFileSync(root+'contact-order-tests.log','utf8').includes('14 passed; 0 failed'));
const v1=read(root+'contact-order-prepare-v1.provenance.json');
assert.equal(hash(fs.readFileSync(v1.source)),v1.source_sha256);

// Archive complete, terminal preparation outputs only. Deduplicate identical
// rerun files by content, while retaining every original path in the manifest.
const archiveRoot=root+'contact-order-evidence-files';fs.mkdirSync(archiveRoot);
const archives=[],seen=new Map();
for(const dir of [old,matched,'runs/contact-order-refined-control-queue']) {
  for(const name of fs.readdirSync(dir).sort()) {
    const p=path.join(dir,name),bytes=fs.readFileSync(p),sha=hash(bytes);
    let archive=seen.get(sha);
    if(!archive) {
      archive=archiveRoot+'/'+sha+'.gz';
      fs.writeFileSync(archive,gzipSync(bytes,{level:9}),{flag:'wx'});
      assert(gunzipSync(fs.readFileSync(archive)).equals(bytes));seen.set(sha,archive);
    }
    archives.push({original:evidence(p),archive:evidence(archive)});
  }
}
const summary=m.items.map(row=>{
  const r=read(row.recipe),c=read(row.conic);
  return {id:row.id,variables:r.variables.length,
    force_variables:r.variables.filter(v=>v.decision.kind!=='motion').length,
    conic_status:c.search?.status??null,feasible:c.report?.sampled_feasible??null,
    maximum_inequality:row.priority_maximum_inequality};
});
const code=['crates/sim-runtime/src/contact_planning.rs',
  ...['joint','joint_timing','joint_orders'].map(n=>'crates/sim-runtime/src/contact_planning/'+n+'.rs'),
  'crates/sim-runtime/examples/prepare_contact_order_neighbors.rs',
  root+'run_contact_order_refinements.mjs',root+'record_contact_orders.mjs'];
const inputs=['contact-order-source.recipe.json','contact-order-source.snapshot.json.gz',
  'contact-order-refinement.search.json','contact-order-refinement.spec.json',
  'contact-order-refined-control.spec.json','contact-order-prepare-v1.rs',
  'contact-order-prepare-v1.provenance.json'].map(n=>root+n);
const logs=['contact-order-tests.log','contact-order-build.log',
  'contact-order-prepare.result.json','contact-order-prepare.log',
  'contact-order-matched-build.log','contact-order-matched-prepare.result.json',
  'contact-order-matched-prepare.log'].map(n=>root+n);
const launches=['runs/contact-order-refinements','runs/contact-order-refined-control'].map(dir=>{
  const spec=read(dir+'/spec.json');
  assert.equal(hash(fs.readFileSync(spec.binary)),spec.binary_sha256);
  return {output_root:dir,spec,context:read(dir+'/context.json'),
    queue:read(dir+'/queue.json'),status:'launched; results incomplete at evidence capture'};
});
write(root+'contact-order-evidence.json',{
  version:1,recorded_at:new Date().toISOString(),
  base_commit:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),
  code:code.map(evidence),inputs:inputs.map(evidence),logs:logs.map(evidence),archives,
  preparer:evidence('/Users/elliot/physics-simulator/target/gait-exploration/release/examples/prepare_contact_order_neighbors'),
  scene:evidence(q.scene),markers:evidence(q.markers),
  scene_archive_index:'examples/full-robot/speed-ceiling/evidence-v8-index.json',
  verification:{planner_tests:14,original_replays_equal:7,refined_motion_equal:true},
  prepared:summary,unprepared:q.unprepared_edges,launches,
  scope:'One adjacent contact-order neighborhood and a same-order refinement control. Knot counts differ by order. Full preparation outputs archived; live solver outputs excluded. No MCTS, INSAT, CEM, faster executed gait, or global maximum is claimed.'
});
console.log(JSON.stringify({original_files:archives.length,unique_archives:seen.size,
  compressed_bytes:[...seen.values()].reduce((n,p)=>n+fs.statSync(p).size,0),verified:true}));
