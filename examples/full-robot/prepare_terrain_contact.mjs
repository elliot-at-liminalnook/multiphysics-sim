// A declared dynamics profile; physical CAD properties and controller stay intact.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const output=process.argv[2]||'examples/full-robot/browser-terrain-contact';
const source='examples/full-robot/browser-reversal';
const scene=JSON.parse(readFileSync(`${source}/scene.json`));
scene.options.omit_inter_link_contact=true;
mkdirSync(output,{recursive:true});
writeFileSync(`${output}/scene.json`,JSON.stringify(scene)+'\n');
const unsafe=JSON.parse(readFileSync(`${source}/short.config.json`));
unsafe.policy.step_reference.sequence.command_postures[0].support_offsets_m[1][0]=-.020;
writeFileSync(`${output}/unsafe-posture.config.json`,JSON.stringify(unsafe)+'\n');

const paths=[`${source}/scene.json`,`${source}/config.json`,`${source}/short.config.json`,`${source}/task.json`,'examples/full-robot/prepare_terrain_contact.mjs'];
writeFileSync(`${output}/manifest.json`,JSON.stringify({version:1,source,
 inputs:Object.fromEntries(paths.map(p=>[p,createHash('sha256').update(readFileSync(p)).digest('hex')])),
 scope:'Experimental dynamics reduction: floor/height-field contact retained; all link-to-link forces omitted. Source masses, geometry, joints, actuators and physical parameters retained. Original numerical tolerances. Online reference and observed-pose overlap checks remain at controller samples; offline recorded-pose geometry audits are required. No inter-link impact response or between-sample collision guarantee.'},null,2)+'\n');
console.log(output);
