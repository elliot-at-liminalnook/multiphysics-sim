// Explicit, unpromoted numerical-precision experiment; source CAD is unchanged.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const output=process.argv[2]||'examples/full-robot/precision-experiment';
const source='examples/full-robot/browser-reversal';
const read=p=>JSON.parse(readFileSync(p));
mkdirSync(output,{recursive:true});
for(const [name,tolerance] of [['1e-6',1e-6],['1e-5',1e-5],['1e-4',1e-4]]){
 for(const [duration,file] of [['short','short.config.json'],['long','config.json']]){
  const config=read(`${source}/${file}`);
  config.implicit.newton.absolute_tolerance=tolerance;
  config.implicit.newton.relative_tolerance=tolerance;
  writeFileSync(`${output}/${name}-${duration}.config.json`,JSON.stringify(config)+'\n');
 }
}
const paths=[`${source}/scene.json`,`${source}/short.config.json`,`${source}/config.json`,`${source}/task.json`,`${source}/forward-reverse.actions.json`,`${source}/sustained.actions.json`,'examples/full-robot/prepare_precision.mjs'];
writeFileSync(`${output}/manifest.json`,JSON.stringify({version:1,source,inputs:Object.fromEntries(paths.map(p=>[p,createHash('sha256').update(readFileSync(p)).digest('hex')])),scope:'Unpromoted precision experiments. Only Newton absolute and relative tolerances change; timestep, controller, physical definitions and task acceptance budgets remain unchanged. 1e-4 failed convergence during the first lift; 1e-5 still missed the rendered 20 ms latency target.'},null,2)+'\n');
console.log(output);
