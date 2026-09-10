import fs from 'node:fs';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));
const cases=read(`${d}/clearance-summary.json`).rows.filter(r=>/^(belt-|integral-|flat-)/.test(r.name));
const rateTargets=cases.filter(r=>fs.existsSync(`${r.prefix}.capability.json`)).map(r=>{const c=read(`${r.prefix}.capability.json`);return {name:r.name,rate_budget_m_s:c.reference_cycle_rate_budget_speed_m_s,limiting_coordinates:c.reference_cycle_coordinates.filter(m=>m.time_scaled_rate_budget_speed_m_s!==null&&m.time_scaled_rate_budget_speed_m_s<=c.reference_cycle_rate_budget_speed_m_s*(1+1e-10)).map(m=>m.coordinate)};});
const validations=read(`${d}/validation-summary.json`).rows.filter(r=>/^(belt-|flat-)/.test(r.name)).map(r=>{
 const file=`${d}/${r.name}.planned-geometry-summary.json`,g=fs.existsSync(file)?read(file):null;
 return {name:r.name,step_s:r.step_s,speeds_m_s:r.windows.map(w=>w.speed_m_s),maximum_slip_ratio:r.maximum_slip_ratio,turn_rad:r.turn_rad,maximum_tilt_rad:r.maximum_tilt_rad,
  stops:r.stops,passed_control_checks:r.passed_control_checks,passed_contact_quality:r.passed_contact_quality,planned_lifts:g?.planned_lifts??null,passed_planned_lifts:g?.passed_planned_lifts??null,inter_link_geometry_audit:g?.inter_link_geometry_audit??null};
});
fs.writeFileSync(`${d}/exploration-summary.json`,JSON.stringify({development_screens:cases.map(r=>({name:r.name,completed:r.completed,speed_m_s:r.speed_m_s,maximum_slip_ratio:r.maximum_slip_ratio,accepted_screen:r.accepted_screen})),rateTargets,validations,
 scope:'New belt posture, CAD COM centering, integral feedback, CAD steering, flat-return shape, stride and lift-height experiments. The 5% slip threshold remains a quality criterion rather than a physical limit. Conditional rate budgets exclude loaded dynamics and are not global maximum-speed claims.'},null,2)+'\n');
console.log({development_screens:cases.length,rate_targets:rateTargets.length,validations:validations.length});
