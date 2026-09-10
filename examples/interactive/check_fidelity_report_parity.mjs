// Check the complete serialized report from native and WASM implementations.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const [nativePath,browserPath,output]=process.argv.slice(2);
assert(output,'usage: check_fidelity_report_parity native-report browser-report fresh-summary');
const a=JSON.parse(fs.readFileSync(nativePath)),b=JSON.parse(fs.readFileSync(browserPath));
let numeric=0,maximum=0,worst='';
function compare(a,b,path){
 if(typeof a==='number'&&typeof b==='number'){
  assert(Number.isFinite(a)&&Number.isFinite(b));const d=Math.abs(a-b);numeric++;
  if(d>maximum){maximum=d;worst=path;}
  assert(d<=1e-15+1e-12*Math.max(Math.abs(a),Math.abs(b)),`report numeric mismatch ${path}`);return;
 }
 if(a&&b&&typeof a==='object'&&typeof b==='object'){
  assert.equal(Array.isArray(a),Array.isArray(b),path);assert.deepEqual(Object.keys(a).sort(),Object.keys(b).sort(),path);
  for(const key of Object.keys(a))compare(a[key],b[key],`${path}/${key}`);return;
 }
 assert.equal(a,b,path);
}
compare(a,b,'');
const summary={passed:true,native_report:nativePath,browser_report:browserPath,numeric_values:numeric,maximum_difference:maximum,worst,
 scope:'Full comparison-report serialization parity, absolute 1e-15 + relative 1e-12; not a physical accuracy tolerance.'};
fs.writeFileSync(output,JSON.stringify(summary,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(summary));
