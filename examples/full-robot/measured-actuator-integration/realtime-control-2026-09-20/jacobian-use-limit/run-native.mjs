import fs from 'node:fs';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
const dir=import.meta.dirname,protocol=JSON.parse(fs.readFileSync(`${dir}/protocol.json`));
for(const kind of ['before','candidate'])assert.equal(createHash('sha256').update(fs.readFileSync(`${dir}/${kind}-native.bin`)).digest('hex'),protocol[`binary_${kind}_sha256`]);
const outcomes=[];
for(const [folder,mode,prefix,kind] of [['','native-fast','before','before'],['','native','candidate','candidate'],...Object.keys(protocol.variants).map(name=>[name,'native','candidate','candidate'])]) {
 const output=resolve(dir,folder);
 assert(!fs.existsSync(`${output}/${prefix}.native.json`),'refusing to overwrite an existing measurement');
 const result=spawnSync(process.execPath,['web/tests/compare-native-wasm.mjs',output,mode,prefix],{stdio:'inherit',env:{...process.env,NATIVE_BINARY:`${dir}/${kind}-native.bin`}});
 outcomes.push({folder,mode,prefix,status:result.status,signal:result.signal});
 fs.writeFileSync(`${dir}/run-outcomes.json`,JSON.stringify(outcomes,null,2)+'\n');
 if(result.status!==0&&!folder)process.exit(result.status??1);
}
