import fs from 'node:fs';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));
const config=read(`${d}/capability-recipe.json`).inspection;
config.record_poses=true;config.samples=[];
for(const row of read(`${d}/clearance-trials.json`).rows.filter(r=>r.planning_exit!==0)){
 const error=fs.readFileSync(`${row.prefix}.plan-error.txt`,'utf8');
 if(!error.startsWith('internal contact'))continue;
 const match=error.match(/independent coordinates (\[[^\]]+\])/);if(!match)throw Error('missing rejection coordinates');
 config.samples.push({id:row.name,coordinates:JSON.parse(match[1])});
}
fs.writeFileSync(`${d}/rejection-probes.json`,JSON.stringify(config,null,2)+'\n');
