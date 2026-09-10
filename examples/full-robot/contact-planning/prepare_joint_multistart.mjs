import fs from 'node:fs';
import {gunzipSync} from 'node:zlib';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const d='examples/full-robot/contact-planning/';
const raw=d+'joint-start-screen.result.jsonl';
const contents=fs.existsSync(raw)?fs.readFileSync(raw):gunzipSync(fs.readFileSync(raw+'.gz'));
const rows=contents.toString('utf8').trim().split('\n').map(x=>JSON.parse(x));
const summary=JSON.parse(fs.readFileSync(d+'joint-start-screen.summary.json'));
const base=JSON.parse(fs.readFileSync(d+'joint-workspace-speed.recipe.json'));
assert.equal(rows.length,257);
const entries=[];
for(const {group,best} of summary.group_best){
  const row=rows.find(r=>r.id===best.id);assert(row?.result);
  const recipe=structuredClone(base);
  recipe.candidate=row.result.candidate;
  recipe.search.maximum_evaluations=3500;
  recipe.search.maximum_outer_iterations=4;
  recipe.search.inner.maximum_iterations=6;
  recipe.search.inner.maximum_evaluations=2200;
  const path=d+'joint-multistart-'+group+'.recipe.json';
  fs.writeFileSync(path,JSON.stringify(recipe,null,2)+'\n',{flag:'wx'});
  entries.push({group,start_id:row.id,path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex'),initial_maximum_inequality:best.maximum_inequality});
}
fs.writeFileSync(d+'joint-multistart-preparation.json',JSON.stringify({entries,variables:base.variables.length,evaluations_per_start:3500,scope:'One minimum-maximum-inequality seed from each of the four timing/body screen groups. Same physical gates, speed target and CAD workspace search bounds as joint-workspace-speed. All motion, timing and force variables remain free. This bounded local multistart does not exhaust the phase grid, gait families or continuous motion space.'},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify(entries));
