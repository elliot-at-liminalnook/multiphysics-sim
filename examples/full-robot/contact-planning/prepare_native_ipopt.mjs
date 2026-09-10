// Obtain only the native solver and its dependencies; never invoke Python.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
assert(process.platform==='darwin'&&process.arch==='x64','This pinned artifact is for macOS x86_64');
const dir='runs/native-ipopt-casadi-3.8.0', wheel=dir+'/casadi.whl';
const url='https://files.pythonhosted.org/packages/2b/f6/fa7d705761915e0e7f243a407b0c44772e15d32aa7d75b82f2411b32684e/casadi-3.8.0-cp311-abi3-macosx_11_0_x86_64.macosx_11_0_intel.whl';
const expected='456eb3b43ca868ac0b46f38526ba4f09201943dc7553e23c212945fce79aa909';
const hash=x=>crypto.createHash('sha256').update(x).digest('hex');
const identity=p=>({path:p,bytes:fs.statSync(p).size,sha256:hash(fs.readFileSync(p))});
fs.mkdirSync(dir,{recursive:true});
if(!fs.existsSync(wheel)){const response=await fetch(url);assert(response.ok);fs.writeFileSync(wheel,Buffer.from(await response.arrayBuffer()),{flag:'wx'});}
assert.equal(hash(fs.readFileSync(wheel)),expected,'Archive identity mismatch');
const entries=new Set(execFileSync('unzip',['-Z1',wheel]).toString().trim().split('\n'));
const extract=entry=>{
  assert(entries.has(entry)&&entry.startsWith('casadi/')&&!entry.includes('..'),'Invalid archive entry');
  const contents=execFileSync('unzip',['-p',wheel,entry],{maxBuffer:128*1024*1024});
  const output=dir+'/extracted/'+entry;fs.mkdirSync(path.dirname(output),{recursive:true});
  if(fs.existsSync(output))assert.equal(hash(fs.readFileSync(output)),hash(contents),'Extracted artifact changed: '+output);
  else fs.writeFileSync(output,contents,{flag:'wx'});
  return output;
};
const pending=['casadi/libipopt.dylib'],seen=new Set(),libraries=[];
while(pending.length){
  const entry=pending.shift();if(seen.has(entry))continue;seen.add(entry);
  const output=extract(entry);const listed=execFileSync('otool',['-L',output]).toString().split('\n').slice(1).map(s=>s.trim().split(' (')[0]).filter(Boolean);
  const install_name=execFileSync('otool',['-D',output]).toString().trim().split('\n')[1];
  assert.equal(listed[0],install_name,'Unexpected Mach-O dependency listing');
  const dependencies=listed.slice(1); // First entry is this image's ID, not a dependency.
  libraries.push({...identity(output),install_name,dependencies});
  for(const dependency of dependencies){
    if(dependency.startsWith('@rpath/')||dependency.startsWith('@loader_path/'))pending.push('casadi/'+path.basename(dependency));
    else assert(dependency.startsWith('/usr/lib/')||dependency.startsWith('/System/'),'Unexpected dependency '+dependency);
  }
}
const headers=['IpStdCInterface.h','IpTypes.h','IpoptConfig.h'].map(n=>extract('casadi/include/coin-or/'+n));
const config=fs.readFileSync(headers[2],'utf8'),api=fs.readFileSync(headers[0],'utf8');
assert(/^#define IPOPT_VERSION "3\.14\.19"$/m.test(config));
assert(!/^\s*#\s*define\s+IPOPT_(INT64|SINGLE)\b/m.test(config));
assert(/typedef bool Bool;/.test(api));
const licenses=[...entries].filter(n=>n.startsWith('casadi/include/licenses/')&&!n.endsWith('/')&&/(ipopt|mumps|metis|openblas|gcc-external)/i.test(n)).map(extract).map(identity);
const manifest={url,archive:identity(wheel),ipopt_version:[3,14,19],abi:{ipnumber_bits:64,ipindex_bits:32,boolean:'C bool'},libraries,headers:headers.map(identity),licenses,platform:process.platform,architecture:process.arch,system:execFileSync('sw_vers').toString(),scope:'Pinned native C library and transitive bundled dependencies from the official CasADi distribution. No Python interpreter, CasADi modeling API or alternative robot runtime is used. Other architectures/build ABIs are not verified.'};
fs.writeFileSync(process.argv[2]??'examples/full-robot/contact-planning/joint-ipopt-native-library.json',JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({libraries:libraries.length,headers:headers.length,licenses:licenses.length,ipopt_version:manifest.ipopt_version,archive_sha256:expected}));
