// Explicit experiment configuration from Rust-resolved CAD/runtime properties.
import fs from 'node:fs';import crypto from 'node:crypto';import assert from 'node:assert/strict';
const root='examples/full-robot/contact-implicit/';
const read=p=>JSON.parse(fs.readFileSync(p));const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const profilePath=root+'resolved-floor-contact.json',profile=read(profilePath);
const sourcePath=root+'surface25-translating-refinement.result.json';
const recipe=read(root+'surface25-translating-refinement.recipe.json');
const capture=read('runs/contact-implicit/surface-tracking-ff64-v3.native.json');
assert.equal(profile.expected_cad_sha256,recipe.config.expected_cad_sha256);
assert.equal(profile.friction_model.kind,'regularized_coulomb');
assert(profile.normal_dissipation_s_m>0);
const friction=[...new Set(profile.links.map(link=>link.kinetic_friction))];assert.equal(friction.length,1,'heterogeneous contact needs explicit per-link models');
const previous=structuredClone(recipe.config.contact);
recipe.config.contact={stiffness_n_m:profile.stiffness_per_sample_n_m,smoothing_m:previous.smoothing_m,
  dissipation_velocity_m_s:1/profile.normal_dissipation_s_m,friction_coefficient:friction[0],stiction_velocity_m_s:profile.friction_model.slip_speed_m_s};
recipe.initial_positions=read(sourcePath).positions;
recipe.stiffness_schedule_n_m=[recipe.config.contact.stiffness_n_m];
recipe.smoothing_schedule_m=[recipe.config.contact.smoothing_m];
const mass=capture.recording.scene.robot.links.reduce((sum,link)=>sum+link.mass,0);
const gravity=-capture.recording.scene.robot.gravity[2];
const duration=(recipe.initial_positions.length-1)*recipe.config.step_s;
const requestedSpeed=Math.hypot(...recipe.config.velocity_reference.slice(0,2));
const scale=.05*friction[0]*mass*gravity*requestedSpeed*duration;
recipe.provenance.physics_correction={resolved_profile:profilePath,profile_sha256:sha(profilePath),previous_contact:previous,
  interpretation:'Use the actual compiled material-pair kinetic friction and normal dissipation. Earlier trials incorrectly treated the world friction scalar as the foot/world coefficient. Pointwise planning friction still differs from the runtime patch model; smoothing still permits force at separation.'};
recipe.provenance.warm_start=sourcePath;recipe.provenance.warm_start_sha256=sha(sourcePath);
for(const work of [false,true]){
  const trial=structuredClone(recipe);if(work)trial.config.contact_sliding_work_scale_j=scale;
  trial.provenance.work_cost=work?{scale_j:scale,formula:'0.05 * mu_kinetic * CAD_mass * g * requested_speed * horizon',mass_kg:mass,gravity_m_s2:gravity,
    scope:'Objective normalization, not a physical speed limit or proof of a 5% per-foot slip ratio. Penalize dissipated tangential contact work; unloaded foot motion remains free.'}:null;
  const name=work?'resolved-contact-work':'resolved-contact-control';
  fs.writeFileSync(root+name+'.recipe.json',JSON.stringify(trial,null,2)+'\n',{flag:'wx'});
}
console.log({contact:recipe.config.contact,sliding_work_scale_j:scale,mass_kg:mass});
