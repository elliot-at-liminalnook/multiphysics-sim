//! Inspect first-step residuals without changing physical or solver tolerances.
use sim_phenomena::scenarios::thermoelastic_damping::{Material, ThermoelasticBeam};
use std::f64::consts::PI;
fn main() {
    if std::env::args().any(|s|s=="--validate") {
        let report=sim_phenomena::scenarios::thermoelastic_damping::run();
        println!("{}",serde_json::to_string(&report).unwrap());
        if !report.passed(){std::process::exit(1);}
        return;
    }
    let unscaled=std::env::args().any(|s|s=="--unscaled");
    let registry=sim_runtime::registry();
    let material=Material::ALUMINIUM;
    let frequency=2.0*PI*10e3;
    let thickness=(PI*PI*material.diffusivity()/frequency).sqrt();
    let h=2.0*PI/frequency/80.0;
    for scale in [1.0/3.0,1.0/3.0_f64.sqrt(),1.0,3.0_f64.sqrt(),3.0] {
        let beam=ThermoelasticBeam { material,thickness:scale*thickness,width:1e-3,layers:16,frequency };
        let mut b=beam.model(&registry);
        for island in &mut b.runtime.islands {
            if unscaled {island.system.set_residual_row_scales(vec![1.0;island.system.reduced_dimension()]).unwrap();}
            island.set_attempt_audit_limit(20);
        }
        let result=b.runtime.advance(h,h);
        let attempts=b.runtime.islands.iter().flat_map(|i|i.implicit_attempts.iter()).collect::<Vec<_>>();
        let rows=attempts.last().map(|a|{
            let mut rows=a.residual.iter().enumerate().map(|(i,r)|(i,*r,a.newton.residual_limits[i])).collect::<Vec<_>>();
            rows.sort_by(|a,b|b.1.abs().total_cmp(&a.1.abs()));rows.truncate(5);rows
        });
        println!("{}",serde_json::json!({"thickness_scale":scale,"error":result.err().map(|e|e.to_string()),
            "layer_conductance_w_per_k":b.layer_conductance,"thermal_subtraction_scale_w":b.layer_conductance*f64::EPSILON*material.temperature,
            "largest_rows":rows,"attempts":attempts,"unscaled":unscaled,
            "row_scales":b.runtime.islands.iter().map(|i|i.system.residual_row_scales()).collect::<Vec<_>>()}));
    }
}
