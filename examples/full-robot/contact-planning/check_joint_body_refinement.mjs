import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const d='examples/full-robot/contact-planning/';
const read=n=>JSON.parse(fs.readFileSync(d+n));
const audit=read('joint-body8-force-jacobian.result.json');
const initial=read('joint-body8-speed-initial.result.json');
// The recipe preparation serializes through JavaScript, which canonicalizes
// signed zero. Compare all other data exactly, and record those zero changes.
assert(isDeepStrictEqual(JSON.parse(JSON.stringify(audit.reference_report)),initial),'Refined report has nonzero numerical or structural differences');
let signedZeros=0;
function count(a,b){if(typeof a==='number'&&typeof b==='number'){if(!Object.is(a,b)){assert(a===0&&b===0);signedZeros++;}}else if(a&&typeof a==='object'){for(const key of Object.keys(a))count(a[key],b[key]);}}
count(audit.reference_report,initial);
assert.equal(audit.cases.length,3);
for(const c of audit.cases){assert.equal(c.checked_force_columns,120);assert.equal(c.fallback_columns,0);assert.equal(c.independent_uncached_probe_matches,2);assert.equal(c.physical_rows,4480);assert(c.max_scaled_error<1e-5);}
const preparation=read('joint-body-refinement-preparation.json');
assert.equal(preparation.matched_original_frames,160);
assert.equal(preparation.refined_frames,168);
assert(preparation.maximum_normalized_common_frame_difference<1e-6);
const verification={exact_report_equality_after_json_signed_zero_canonicalization:true,signed_zero_canonicalizations:signedZeros,checked_force_columns:360,independent_uncached_probe_matches:6,maximum_derivative_scaled_error:Math.max(...audit.cases.map(c=>c.max_scaled_error)),maximum_normalized_common_frame_difference:preparation.maximum_normalized_common_frame_difference,scope:'Initial body-refinement and force-derivative fidelity checks only; the initial candidate is still physically infeasible.'};
fs.writeFileSync(d+'joint-body-refinement-verification.json',JSON.stringify(verification,null,2)+'\n');
console.log(JSON.stringify(verification));
