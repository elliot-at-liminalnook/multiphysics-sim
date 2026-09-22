import {spawnSync} from 'node:child_process';
import {resolve} from 'node:path';
const dir=import.meta.dirname;
for(const [subdir,prefix,port] of [['','browser-before',4191],['','candidate',4192],['analytic','candidate',4192]]) {
 const result=spawnSync(process.execPath,['web/tests/compare-native-wasm.mjs',resolve(dir,subdir),'wasm',prefix],{stdio:'inherit',env:{...process.env,VIEWER_URL:`http://127.0.0.1:${port}`}});
 if(result.status!==0)process.exit(result.status??1);
}
