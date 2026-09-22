import {spawnSync} from 'node:child_process';
import {resolve} from 'node:path';
import fs from 'node:fs';
import {createHash} from 'node:crypto';
const dir=import.meta.dirname;
const protocol=JSON.parse(fs.readFileSync(`${dir}/protocol.json`));
for(const binary of ['before','candidate']) {
 const path=`${dir}/${binary}-native.bin`;
 if(createHash('sha256').update(fs.readFileSync(path)).digest('hex')!==protocol[`binary_${binary}_sha256`])throw Error('binary identity mismatch');
}
for(const [subdir,mode,prefix,binary] of [
 ['', 'native-fast','before','before'],
 ['', 'native','candidate','candidate'],
 ['analytic','native','candidate','candidate'],
]) {
 const result=spawnSync(process.execPath,['web/tests/compare-native-wasm.mjs',resolve(dir,subdir),mode,prefix],{stdio:'inherit',env:{...process.env,NATIVE_BINARY:`${dir}/${binary}-native.bin`}});
 if(result.status!==0)process.exit(result.status??1);
}
