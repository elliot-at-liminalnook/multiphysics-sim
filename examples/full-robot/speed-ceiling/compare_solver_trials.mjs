import fs from 'node:fs';import {isDeepStrictEqual} from 'node:util';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));
const base=read('runs/speed-ceiling/validation/flat125-human-5ms.native.json'),profiled=read('runs/speed-ceiling/performance/flat125-5ms.profiled-native.json');
const physical=c=>c.frames.map(({stepping_wall_s,...frame})=>frame);
const profiling_preserves_values=isDeepStrictEqual(physical(base),physical(profiled))&&isDeepStrictEqual(base.transitions,profiled.transitions)&&isDeepStrictEqual(base.recording,profiled.recording);
if(!profiling_preserves_values)throw Error('profile instrumentation changed reference');
const rows=[];
for(const r of read(`${d}/solver-trials.json`).rows){
 const c=read(`${r.prefix}.native.json`);if(!c.completed||c.error)throw Error(`incomplete ${r.name}`);
 const expected=structuredClone(base.recording);expected.config=structuredClone(c.recording.config);
 if(!isDeepStrictEqual(expected,c.recording))throw Error('solver trial changed recording outside config');
 for(const [key,value] of Object.entries(r.newton_overrides))if(!isDeepStrictEqual(c.recording.config.implicit.newton[key],value))throw Error(`missing declared Newton override ${key}`);
 for(const [key,value] of Object.entries(r.implicit_overrides))if(!isDeepStrictEqual(c.recording.config.implicit[key],value))throw Error(`missing declared implicit override ${key}`);
 // Typed captures normalize omitted defaults; compare values outside declared paths.
 const actual=structuredClone(c.recording.config),original=structuredClone(base.recording.config);
 for(const k of Object.keys(r.newton_overrides)){delete actual.implicit.newton[k];delete original.implicit.newton[k];}
 for(const k of Object.keys(r.implicit_overrides)){delete actual.implicit[k];delete original.implicit[k];}
 if(!isDeepStrictEqual(actual,original))throw Error('undeclared config difference');
 let differences=0,maxError=0,maxFraction=0,compared=0,worstAbsolute=null,worstFraction=null;const failures=[];
 const walk=(a,b,path)=>{
  if(typeof a==='number'&&typeof b==='number') {compared++;const e=Math.abs(a-b),limit=1e-7+1e-8*Math.max(Math.abs(a),Math.abs(b));if(e>maxError)worstAbsolute={path,reference:a,candidate:b,error:e,limit};if(e/limit>maxFraction)worstFraction={path,reference:a,candidate:b,error:e,limit};maxError=Math.max(maxError,e);maxFraction=Math.max(maxFraction,e/limit);if(!Number.isFinite(e)||e>limit){differences++;if(failures.length<12)failures.push({path,reference:a,candidate:b,error:e,limit});}return;}
  if(a!==null&&b!==null&&typeof a==='object'&&typeof b==='object'){
   if(!isDeepStrictEqual(Object.keys(a).sort(),Object.keys(b).sort())){differences++;if(failures.length<12)failures.push({path,reason:'structure mismatch'});return;}
   for(const k of Object.keys(a))walk(a[k],b[k],`${path}.${k}`);return;
  }if(!Object.is(a,b)){differences++;if(failures.length<12)failures.push({path,reference:a,candidate:b});}
 };
 walk(physical(base),physical(c),'frames');walk(base.transitions,c.transitions,'transitions');
 let maxBody=0;for(let i=0;i<base.frames.length;i++){const a=base.frames[i].poses.find(p=>p.name.includes('Chassis')),b=c.frames[i].poses.find(p=>p.name.includes('Chassis'));maxBody=Math.max(maxBody,Math.hypot(...a.position_m.map((v,j)=>v-b.position_m[j])));}
 const profile=read(`${r.prefix}.profile.json`);
 rows.push({name:r.name,compared_numbers:compared,maximum_numeric_error:maxError,maximum_tolerance_fraction:maxFraction,worst_absolute:worstAbsolute,worst_tolerance_fraction:worstFraction,maximum_body_difference_m:maxBody,differences,failures,passed:differences===0,profile_wall_s:profile.wall_s,profile_buckets:profile.buckets.filter(b=>b.seconds>.1)});
}
const result={profiling_preserves_values,rows,numeric_tolerance:{absolute:1e-7,relative:1e-8},scope:'Every recorded physical value and task transition compared, excluding only per-frame wall time. Scene, inputs, seeds and non-overridden config values verified. Profiling times are diagnostic with concurrent work, not performance acceptance.'};
fs.writeFileSync(`${d}/solver-comparison.json`,JSON.stringify(result,null,2)+'\n');console.log(rows.map(({name,passed,differences,maximum_body_difference_m,maximum_tolerance_fraction})=>({name,passed,differences,maximum_body_difference_m,maximum_tolerance_fraction})));
