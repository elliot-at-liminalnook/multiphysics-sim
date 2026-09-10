// Experiment configuration only; shared Rust owns all kinematics and dynamics.
import fs from 'node:fs';
import crypto from 'node:crypto';

const dir = 'examples/full-robot/contact-planning/';
const source = 'examples/full-robot/speed-ceiling/posture-inspection.json';
const inspection = JSON.parse(fs.readFileSync(source));
const baseline = JSON.parse(fs.readFileSync(dir + 'phase-pairs.recipe.json'));
const rows = inspection.rows.filter(r => !r.error && !r.sampled_penetrations.length
  && !r.authored_limit_violations.length);
if (inspection.cad_sha256 !== baseline.robot.expected_cad_sha256 || !rows.length)
  throw Error('matching CAD inspection with admissible posture samples required');
const anchor = rows.find(r => r.id === 'hip0-foot-60');
if (!anchor) throw Error('missing original posture for relative base height');
const margin = 0.02;
const meanZ = r => r.markers.reduce((s, m) => s + m.position_world_m[2], 0) / r.markers.length;
const seeds = ['hip30-foot-60', 'hip45-foot-60'];
for (const id of seeds) {
  const row = rows.find(r => r.id === id);
  if (!row || row.markers.length !== baseline.motion.feet.length)
    throw Error(`missing compatible posture ${id}`);
  const recipe = structuredClone(baseline);
  recipe.robot.initial_coordinates = row.coordinates;
  recipe.robot.initial_base_translation_m[2] += meanZ(anchor) - meanZ(row);
  recipe.motion.feet.forEach((foot, i) => {
    if (row.markers[i].id !== anchor.markers[i].id) throw Error('marker ordering mismatch');
    foot.center_world_m[0] = row.markers[i].position_world_m[0];
    foot.center_world_m[1] = row.markers[i].position_world_m[1];
  });
  for (const variable of recipe.variables) {
    const d = variable.decision;
    if (d.kind === 'foot_center' && d.axis < 2) {
      const values = rows.map(r => r.markers[d.foot].position_world_m[d.axis]);
      variable.bound.lower = Math.min(...values) - margin;
      variable.bound.upper = Math.max(...values) + margin;
    }
  }
  fs.writeFileSync(dir + 'posture-' + id + '.recipe.json', JSON.stringify(recipe, null, 2) + '\n', { flag: 'wx' });
}
fs.writeFileSync(dir + 'posture-starts.json', JSON.stringify({
  source, source_sha256: crypto.createHash('sha256').update(fs.readFileSync(source)).digest('hex'),
  cad_sha256: inspection.cad_sha256, inspected_postures: rows.map(r => r.id), seeds,
  foothold_box_margin_m: margin,
  scope: 'Numerical x/y search box enclosing CAD-derived footholds of 20 previously inspected postures, plus an explicit 2 cm fringe. The box and paths between postures are not certified collision-free or globally complete. Joint search intervals, physical tolerances and the weak speed penalty used for feasibility restoration are unchanged. These are optimizer initializations, not manually authored new gaits.'
}, null, 2) + '\n', { flag: 'wx' });
