// Reproducible acceptance orchestration; all parameterization and execution are Rust.
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import assert from 'node:assert/strict';
const [capture,recipe,identity,changed,directory,bundle,prefix,binaries='target/release/examples']=process.argv.slice(2);
assert(prefix,'usage: run_motion_case capture recipe identity-values changed-values fresh-directory isolated-bundle preset-prefix [binary-directory]');
function run(command,args,output){
 const fd=output?fs.openSync(output,'wx'):null;
 const result=spawnSync(command,args,{stdio:['ignore',fd??'pipe','pipe']});
 if(fd!==null)fs.closeSync(fd);
 assert(result.status===0,`${path.basename(command)} failed: ${String(result.stderr??result.error??'').slice(0,400)}`);
 if(result.stdout?.length)process.stdout.write(result.stdout);
}
run(path.join(binaries,'prepare_motion_experiment'),[capture,directory]);
for(const [source,name] of [[recipe,'parameterization'],[identity,'identity-values'],[changed,'changed-values']]){
 fs.copyFileSync(source,path.join(directory,name+'.json'),fs.constants.COPYFILE_EXCL);
}
const file=name=>path.join(directory,name+'.json');
run(path.join(binaries,'run_environment'),[file('scene'),file('config'),file('task'),file('actions')],file('base-native'));
for(const candidate of ['identity','changed']){
 run(path.join(binaries,'materialize_motion'),[file('scene'),file('actions'),file('parameterization'),file(candidate+'-values'),file(candidate+'-motion')]);
 run(path.join(binaries,'run_environment'),['--motion',file(candidate+'-motion'),file('config'),file('task')],file(candidate+'-native'));
}
run(process.execPath,['examples/interactive/prepare_motion_browser_case.mjs',directory,bundle,prefix]);
