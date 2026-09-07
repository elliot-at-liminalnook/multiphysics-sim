import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const inputs={};
async function read(p){const b=await readFile(p);inputs[p]=createHash('sha256').update(b).digest('hex');return JSON.parse(b);}
const recipe=await read('examples/full-robot/sample-reuse-experiment.json');
await read(recipe.scene);const baseline=await read(recipe.config);assert(baseline.implicit.reuse_step_jacobian);assert(!baseline.embedding.block_dependent_factorization);baseline.profile_solver=true;
const candidate=structuredClone(baseline);candidate.implicit.reuse_controller_sample_jacobian=true;
const viewer=structuredClone(candidate);viewer.profile_solver=false;
const refined=structuredClone(candidate);refined.step_s/=2;refined.steps*=2;refined.report_every*=2;
const refinedReference=structuredClone(refined);delete refinedReference.implicit.reuse_controller_sample_jacobian;
// Stop just after the first observed subdivision divergence. These diagnostic
// runs intentionally finish before the motion program and report that cutoff.
const diagnostic=structuredClone(refined),diagnosticReference=structuredClone(refinedReference);
const diagnosticSteps=1.52/refined.step_s;
assert(Number.isInteger(diagnosticSteps));
diagnostic.steps=diagnosticReference.steps=diagnosticSteps;
const newtonAudit=structuredClone(diagnostic);
newtonAudit.implicit.newton_audit_window_s=[1.5198,1.519875];
await mkdir(recipe.output_directory,{recursive:true});const outputs={};
for(const [name,value] of [['reference.config.json',baseline],['config.json',candidate],['viewer.config.json',viewer],['refined.config.json',refined],['refined.reference.config.json',refinedReference],['diagnostic.config.json',diagnostic],['diagnostic.reference.config.json',diagnosticReference],['newton-audit.config.json',newtonAudit]]){
 const b=JSON.stringify(value);outputs[name]=createHash('sha256').update(b).digest('hex');await writeFile(recipe.output_directory+'/'+name,b);
}
inputs['examples/full-robot/prepare_sample_reuse.mjs']=createHash('sha256').update(await readFile('examples/full-robot/prepare_sample_reuse.mjs')).digest('hex');
await writeFile(recipe.output_directory+'/manifest.json',JSON.stringify({recipe,inputs,outputs},null,2));
console.log('Prepared controller-sample reuse experiment with unchanged physical model.');
