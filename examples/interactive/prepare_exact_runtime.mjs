// Preserve an existing experiment's scene/configurations for an implementation-only comparison.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [directory,parentDirectory]=process.argv.slice(2);
assert(parentDirectory,'usage: prepare_exact_runtime.mjs output-directory parent-experiment-directory');
const hash=b=>createHash('sha256').update(b).digest('hex');
const inputs={},outputs={},comparisons={};
async function read(p){const b=await readFile(p);inputs[p]=hash(b);return JSON.parse(b);}
const parent=await read(parentDirectory+'/manifest.json');
await mkdir(directory,{recursive:true});
for(const name of Object.keys(parent.comparisons)){
 assert(/^[a-z][a-z0-9_-]*$/.test(name),'invalid case name');
 const p=`${parentDirectory}/${name}.config.json`,b=await readFile(p);inputs[p]=hash(b);
 const out=`${directory}/${name}.config.json`;await writeFile(out,b);outputs[out]=hash(b);
 comparisons[name]=`${parentDirectory}/${name}.execution.json`;
 const expected=await read(comparisons[name]);assert(expected.completed&&expected.error===null);
}
inputs[parent.scene]=hash(await readFile(parent.scene));
inputs[import.meta.filename]=hash(await readFile(import.meta.filename));
await writeFile(directory+'/manifest.json',JSON.stringify({scene:parent.scene,inputs,outputs,comparisons,
 scope:'Implementation-only experiment. Identical scene/configurations; require exact full frames, events, solver diagnostics, contact stages and impulses against every parent capture. Timing and browser acceptance remain separate.'},null,2));
console.log('Prepared exact runtime comparisons: '+Object.keys(comparisons).join(', '));
