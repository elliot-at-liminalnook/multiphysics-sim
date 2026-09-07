// Exact full-run comparison for an implementation-only experiment.
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const [directory]=process.argv.slice(2);
assert(directory,'usage: verify_exact_runtime.mjs experiment-directory');
const inputs={};
async function bytes(p){const b=await readFile(p);inputs[p]=createHash('sha256').update(b).digest('hex');return b;}
async function read(p){return JSON.parse(await bytes(p));}
const manifest=await read(directory+'/manifest.json');
for(const [p,h] of Object.entries({...manifest.inputs,...manifest.outputs})){await bytes(p);assert.equal(inputs[p],h,`Changed experiment input: ${p}`);}
const sections=['frames','terminal_frame','hybrid_steps','hybrid_solves','contact_steps','contact_impulses'];
const metadata=['source','scene_options','world','embedding','implicit','independent_coordinates','motor_state_layout','motor_experiment','applied_generalized_loads','policy_experiment','motion_gate','initial_coordinates','initial_base_translation_m'];
const cases=[];
for(const [name,reference] of Object.entries(manifest.comparisons)){
 assert(/^[a-z][a-z0-9_-]*$/.test(name),'invalid case name');
 const candidate=`${directory}/${name}.execution.json`;
 const a=await read(candidate),b=await read(reference);
 assert(a.completed&&a.error===null&&b.completed&&b.error===null);
 const exact={};
 for(const k of sections){assert(k in a&&k in b,`Missing ${k}`);exact[k]=isDeepStrictEqual(a[k],b[k]);assert(exact[k],`${name}: nonidentical ${k}`);}
 for(const k of metadata)assert(isDeepStrictEqual(a[k],b[k]),`${name}: different metadata ${k}`);
 assert.equal(a.simulated_s,b.simulated_s);assert.equal(a.step_s,b.step_s);
 cases.push({name,candidate,reference,exact,step_s:a.step_s,simulated_s:a.simulated_s,
  wall_s:a.stepping_wall_s,parent_wall_s:b.stepping_wall_s,profile:a.solver_profile,parent_profile:b.solver_profile});
}
await bytes(import.meta.filename);
await writeFile(directory+'/verification.json',JSON.stringify({passed:true,
 scope:'Exact recorded physical/numerical results and experiment metadata. Timings may overlap other work; isolated benchmark, browser and task/hardware acceptance are separate.',cases,inputs},null,2));
console.log(JSON.stringify({passed:true,cases:cases.map(c=>c.name),hashed_inputs:Object.keys(inputs).length}));
