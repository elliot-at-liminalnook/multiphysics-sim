// Numerical basis/seed preparation only; shared Rust owns motion and physics.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const root='examples/full-robot/contact-implicit/';
for(const mode of ['uniform','perturbed']) {
  const sourcePath=root+`periodic-${mode}.recipe.json`;
  const bytes=fs.readFileSync(sourcePath), recipe=JSON.parse(bytes);
  const stride=3, oldK=recipe.initial_positions.length-1;
  assert.equal(oldK,24);
  const select=rows=>rows.filter((_,k)=>k%stride===0);
  recipe.initial_positions=select(recipe.initial_positions);
  recipe.config.position_reference=select(recipe.config.position_reference);
  recipe.bounds=[...recipe.bounds.slice(0,-1).filter((_,k)=>k%stride===0),recipe.bounds.at(-1)];
  recipe.config.step_s*=stride;
  recipe.config.periodic_cubic_subdivisions=4;
  recipe.provenance={...recipe.provenance,source_recipe:sourcePath,
    source_recipe_sha256:createHash('sha256').update(bytes).digest('hex'),
    operation:'Eight unique periodic cubic B-spline controls plus XY drift, with analytic position, velocity and acceleration evaluated at four collocation samples per control interval by shared Rust. No contact schedule. Controls are not interpolated poses.',
    numerical_basis:'Downsample the existing constant or deterministic three-harmonic seed every third control. This changes the numerical motion basis, not CAD properties or hardware limits. Finer collocation and basis refinement remain necessary.',
    physical_parameters:'Same resolved friction, stiffness, actuator envelopes, period, target, objective scales and acceptance gates as source.'};
  assert.equal(recipe.initial_positions.length,9);
  assert.equal(recipe.bounds.length,9);
  for(const [suffix,data] of [['recipe',recipe],['initial',{positions:recipe.initial_positions}]])
    fs.writeFileSync(root+`smooth8-${mode}.${suffix}.json`,JSON.stringify(data,null,2)+'\n',{flag:'wx'});
}
