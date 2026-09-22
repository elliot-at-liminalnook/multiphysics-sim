import fs from 'node:fs';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const dir=import.meta.dirname,protocol=JSON.parse(fs.readFileSync(`${dir}/protocol.json`));
assert.equal(createHash('sha256').update(fs.readFileSync(`${dir}/candidate-native.bin`)).digest('hex'),protocol.binary_candidate_sha256);
const outcomes=[];
for(const limit of [16,32,128]) {
 const folder=`uses-${limit}`;
 assert(!fs.existsSync(`${dir}/${folder}/candidate.native.json`),'refusing to overwrite a measurement');
 const result=spawnSync(process.execPath,['web/tests/compare-native-wasm.mjs',`${dir}/${folder}`,'native','candidate'],{stdio:'inherit',env:{...process.env,NATIVE_BINARY:`${dir}/candidate-native.bin`}});
 outcomes.push({folder,status:result.status,signal:result.signal});fs.writeFileSync(`${dir}/shorter-outcomes.json`,JSON.stringify(outcomes,null,2)+'\n');
}
