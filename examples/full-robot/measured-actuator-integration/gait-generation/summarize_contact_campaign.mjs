// Read saved detailed-simulation reports; never infer feasibility from a plan.
import fs from 'node:fs';
import crypto from 'node:crypto';
const [root,out]=process.argv.slice(2);if(!out)throw Error('summarize_contact_campaign.mjs CAMPAIGN NEW_REPORT');
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const directories=[];function walk(p,depth){if(depth>3)return;const entries=fs.readdirSync(p,{withFileTypes:true});if(entries.some(e=>e.name==='report.json'||e.name==='geometry-report.json'))directories.push(p);for(const e of entries)if(e.isDirectory())walk(p+'/'+e.name,depth+1);}walk(root,0);
const protocol=JSON.parse(fs.readFileSync(root+'/validation-protocol.json'));
const results=directories.map(directory=>{const path=directory+(fs.existsSync(directory+'/geometry-report.json')?'/geometry-report.json':'/report.json');const r=JSON.parse(fs.readFileSync(path));const required=['completed','not_fallen','upright','command_bounds','tracking','travel','foot_lift','sampled_collision'];const pass=required.every(k=>r.checks[k]===true);return{directory:directory.slice(root.length+1),path,sha256:sha(path),horizon_s:r.horizon_s,speed_m_s:r.net_speed_m_s,worst_motor_rms_degrees:Math.max(...r.motors.map(m=>m.rms_tracking_degrees)),checks:r.checks,screen_walking_checks_pass:pass,speed_goal_met:r.net_speed_m_s>=protocol.goal_speed_m_s,ten_second_gates_pass:r.horizon_s===10&&r.passes_all_finalist_checks,thirty_second_gates_pass:r.horizon_s===30&&r.passes_all_finalist_checks};});
results.sort((a,b)=>b.speed_m_s-a.speed_m_s);
fs.writeFileSync(out,JSON.stringify({scope:'Candidate screening and validation ledger. Short-run success is not promotion. Numerical, stop/reversal, replay, and long-run gates are separate.',protocol_sha256:sha(root+'/validation-protocol.json'),results,promoted_candidate:null},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify(results.map(r=>({candidate:r.directory,speed:r.speed_m_s,tracking:r.worst_motor_rms_degrees,walking_checks:r.screen_walking_checks_pass,speed_goal:r.speed_goal_met}))));
