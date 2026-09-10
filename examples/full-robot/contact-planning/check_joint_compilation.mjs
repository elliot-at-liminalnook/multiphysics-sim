import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as eq} from 'node:util';
import crypto from 'node:crypto';
const d='examples/full-robot/contact-planning/';
const read=p=>JSON.parse(fs.readFileSync(d+p));
const a=read('return-x25-reference.result.json'),b=read('joint-x25-warm-reference.result.json');
const r=read('joint-x25-warm-reference.recipe.json');
const unchanged=['motion','trajectory','static_feedforward','pause_windows_s','initial_coordinates'];
for(const k of unchanged)assert(eq(a[k],b[k]),'Unexpected motion/static change: '+k);
assert(eq(b.joint_force_reference.forward_reverse_templates,r.force_templates));
assert(!eq(a.dynamic_feedforward,b.dynamic_feedforward));
assert(!eq(a.velocity_feedforward,b.velocity_feedforward));
assert(!b.required_load_audits_passed&&b.diagnostic_allow_failed_reference_audit);
const defaultSame=fs.readFileSync(d+'joint-seed-parent-recompiled.result.json').equals(fs.readFileSync(d+'xhip-lift-reference.result.json'));
assert(defaultSame,'Default compiler regression');
const output={passed:true,unchanged,force_templates_preserved:true,dynamic_and_odd_feedforward_changed:true,
  failed_physical_audits_preserved:true,default_compiler_byte_identical:defaultSame,
  inputs:['return-x25-reference.result.json','joint-x25-warm-reference.result.json','joint-x25-warm-reference.recipe.json',
    'joint-seed-parent-recompiled.result.json','xhip-lift-reference.result.json'].map(p=>({path:d+p,
      sha256:crypto.createHash('sha256').update(fs.readFileSync(d+p)).digest('hex')})),
  scope:'Reference compilation behavior checks only; no detailed runtime force tracking or gait qualification.'};
fs.writeFileSync(d+'joint-force-compiler-checks.json',JSON.stringify(output,null,2)+'\n');
console.log(JSON.stringify({passed:true,default_compiler_byte_identical:defaultSame}));
