// Updating evidence text can preserve a tested runtime and every exact recipe.
import {readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [tested,delivery,uiPath,output]=process.argv.slice(2);assert(output,'usage: verify_metadata_package tested-bundle delivery-bundle ui-report output');
const read=p=>JSON.parse(readFileSync(p)),hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const source=path=>({path,sha256:hash(path)}),ui=read(uiPath);assert(ui.passed);
for(const name of ['leaderboard.json','build-manifest.json']){
  const path=join(tested,name);assert.equal(hash(path),ui.sources.find(s=>resolve(s.path)===resolve(path))?.sha256,'UI report must bind the tested package');
}
const a=read(join(tested,'catalog.json')),b=read(join(delivery,'catalog.json'));
assert.deepEqual(a.presets.map(p=>p.id),b.presets.map(p=>p.id));
for(const preset of a.presets){
  const other=b.presets.find(p=>p.id===preset.id);assert.equal(hash(join(tested,preset.path)),hash(join(delivery,other.path)),preset.id);
}
for(const recipe of ui.recipes){
  const preset=b.presets.find(p=>p.id===`tested-${recipe.id}`);assert.equal(preset.asset_sha256,recipe.asset_sha256);
}
const code=path=>readdirSync(path,{recursive:true}).filter(p=>/\.(?:m?js|css|html|wasm)$/.test(p)).sort();
assert.deepEqual(code(tested),code(delivery));
for(const path of code(tested))assert.equal(hash(join(tested,path)),hash(join(delivery,path)),path);
const report={version:1,passed:true,recipes:ui.recipes,verified_code_files:code(delivery),
  sources:[uiPath,join(tested,'build-manifest.json'),join(delivery,'build-manifest.json'),join(delivery,'leaderboard.json'),import.meta.filename].map(source),
  scope:'Delivery has identical UI/worker/WASM code and all recipe bytes from the successful UI-tested bundle. Only evidence metadata changed; this check does not add physics or timing evidence.'};
writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log({passed:true,recipes:ui.recipes.length});
