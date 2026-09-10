import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as eq} from 'node:util';
const d='examples/full-robot/contact-planning/',read=n=>JSON.parse(fs.readFileSync(d+n));
const pilot=read('joint-servo-command-pilot2000.result.json'),map=read('joint-servo-conic-map.result.json'),fit=read('joint-servo-conic.result.json');
assert.equal(pilot.search.native_status,-1);assert.equal(pilot.model_budget_exhausted,false);assert(pilot.search.final_evaluation);
assert(eq(pilot.initial_report,read('joint-servo-command-jacobian.result.json').reference_report));
assert(eq(pilot.search.final_evaluation.constraints,pilot.report.constraints.inequalities));
assert(eq(map.reference_report,pilot.report));assert(map.duplicate_rejected);
assert.equal(map.cases.length,2);for(const c of map.cases){assert.equal(c.rows,6576);assert(c.baseline_cache_byte_equal);assert(c.bounded_probe_errors.length===3&&c.bounded_probe_errors.every(e=>e<=1e-8));}
assert.equal(map.cases[0].columns,366);assert.equal(map.cases[1].columns,183);
assert(fs.readFileSync(d+'joint-servo-conic-legacy.result.json').equals(fs.readFileSync(d+'joint-conic-frictionless-replay.result.json')));
assert.equal(fit.servo_command_rows,6576);assert.equal(fit.search.status,'PrimalInfeasible');assert.equal(fit.search.has_primal_candidate,false);assert.equal(fit.candidate,null);assert.equal(fit.report,null);
const source=read('joint-servo-command.recipe.json'),recipe=read('joint-servo-conic.recipe.json');source.candidate=pilot.candidate;assert(eq(source,recipe),'Unexpected conic recipe change');
assert(eq(recipe,read('joint-servo-command-speed.recipe.json')),'Full joint search did not retain checked motion, bounds and command limits');
const out={pilot:{models:pilot.model_evaluations,native_status:pilot.search.native_status,initial_report_matches:true,native_final_constraints_match:true},map_cases:map.cases,maximum_command_map_error:Math.max(...map.cases.flatMap(c=>c.bounded_probe_errors)),legacy_conic_byte_identical:true,conic_status:fit.search.status,conic_iterations:fit.search.iterations,servo_command_rows:6576,primal_candidate:false,next_step:'Full joint body/foot/timing/force speed optimization with explicit command limits.',scope:'The fixed-motion conic solver reports infeasibility under the original force boxes, circular friction cones and hard nominal command limits. This is floating-point solver evidence, not an interval certificate, a global physical speed ceiling or a runtime gait.'};
fs.writeFileSync(d+'joint-servo-conic-verification.json',JSON.stringify(out,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(out));
