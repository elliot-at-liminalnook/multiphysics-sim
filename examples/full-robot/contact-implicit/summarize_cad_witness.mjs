import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const target=read('slip-coupled-cad-probe.result.json'),both=read('slip-coupled-cad-both.result.json');
const stripped=structuredClone(both);
for(const f of stripped.frames)for(const p of f.contact_point_probes){delete p.source_link;delete p.source_brep_members;}
assert(same(target,stripped),'two-sided inspection changed prior target results');
assert.equal(both.frames.length,1);assert.equal(both.pair_distances_measured,false);
const probes=both.frames[0].contact_point_probes;
const exact=probes.filter(p=>p.runtime_contact);assert.equal(exact.length,1);
const nearest=members=>members.reduce((a,b)=>a.surface_distance_mm<b.surface_distance_mm?a:b);
const classify=members=>({nearest_boundary:nearest(members),
  inside_members:members.filter(m=>m.solid_states.includes('inside')).map(m=>m.name),
  boundary_members:members.filter(m=>m.solid_states.includes('boundary')).map(m=>m.name),
  unknown_members:members.filter(m=>m.solid_states.includes('unknown')).map(m=>m.name)});
const cells=probes.filter(p=>p.sdf_cell_node);assert.equal(cells.length,8);
const weight=cells.reduce((s,p)=>s+p.sdf_cell_node.weight,0);assert(Math.abs(weight-1)<1e-14);
const interpolated=cells.reduce((s,p)=>s+p.sdf_cell_node.weight*p.sdf_cell_node.stored_distance_m,0);
assert(Math.abs(interpolated+exact[0].runtime_contact.penetration_m)<1e-14,'grid nodes do not reproduce Rust witness');
console.log(JSON.stringify({time_s:both.frames[0].time_s,point_world_m:exact[0].point_world_m,
  reported_grid_penetration_m:exact[0].runtime_contact.penetration_m,
  source_link:exact[0].source_link,source:classify(exact[0].source_brep_members),
  target_link:exact[0].target,target:classify(exact[0].brep_members),
  surrounding_grid_nodes:cells.map(p=>({index:p.sdf_cell_node.index,weight:p.sdf_cell_node.weight,
    stored_distance_m:p.sdf_cell_node.stored_distance_m,target:classify(p.brep_members)})),
  target_results_identical:true,
  scope:'Exact B-rep solid classification and nearest member-boundary distance for one Rust planned-pose witness and eight grid nodes. Whole-part distances are unmeasured. A sampled source point is not necessarily on the exact source boundary; neither a single witness nor grid interpolation proves continuous pair clearance, speed optimality or hardware accuracy.'},null,2));
