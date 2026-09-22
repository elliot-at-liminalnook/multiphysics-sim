import {spawnSync} from 'node:child_process';
import fs from 'node:fs';
import assert from 'node:assert/strict';
const dir=import.meta.dirname;
const protocol=JSON.parse(fs.readFileSync(`${dir}/protocol.json`));
for(const [folder,prefix,port] of [['','browser-before',4192],...['',...Object.keys(protocol.variants)].map(folder=>[folder,'candidate',4194])]) {
 const output=folder?`${dir}/${folder}`:dir;
 assert(!fs.existsSync(`${output}/${prefix}.wasm.json`),'refusing to overwrite a worker measurement');
 const result=spawnSync(process.execPath,['web/tests/compare-native-wasm.mjs',output,'wasm',prefix],{stdio:'inherit',env:{...process.env,VIEWER_URL:`http://127.0.0.1:${port}`}});
 if(result.status!==0)process.exit(result.status??1);
}
