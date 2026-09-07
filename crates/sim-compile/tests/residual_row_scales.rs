use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld};
use sim_domain_rotational::elements as rot;
use sim_dynamics::{Integrator, JacobianParts, System};

fn oscillator() -> Runtime {
    let mut registry=BehaviorRegistry::default();rot::register(&mut registry).unwrap();
    let mut model=ModelWorld::default();
    let inertia=model.part(&registry,"rotor",rot::INERTIA,[("inertia",2.0),("initial.angle",0.3)]).unwrap();
    let spring=model.part(&registry,"spring",rot::SPRING,[("stiffness",8.0)]).unwrap();
    let wall=model.part(&registry,"wall",rot::GROUND,[]).unwrap();
    model.connect([inertia.port("shaft"),spring.port("a")]);
    model.connect([spring.port("b"),wall.port("flange")]);
    Runtime::new(model,&registry,Integrator::implicit_midpoint()).unwrap()
}

#[test]
fn explicit_equation_units_scale_both_derivatives_and_residuals_atomically() {
    let mut runtime=oscillator();let system=&mut runtime.islands[0].system;
    let state=system.reduced_initial();let rate=vec![0.2;state.len()];
    let mut before=vec![0.0;state.len()];system.residual(0.0,&state,&rate,&mut before);
    let mut jac=JacobianParts::default();assert!(system.jacobian(0.0,&state,&rate,&mut jac));
    let scales=(0..state.len()).map(|i|if i%2==0 {1e-3}else{1e3}).collect::<Vec<_>>();
    system.set_residual_row_scales(scales.clone()).unwrap();
    for bad in [vec![],vec![0.0;state.len()],vec![f64::NAN;state.len()]] {
        assert!(system.set_residual_row_scales(bad).is_err());
        assert_eq!(system.residual_row_scales(),Some(scales.as_slice()));
    }
    let mut after=vec![0.0;state.len()];system.residual(0.0,&state,&rate,&mut after);
    for (i,(a,b)) in after.iter().zip(&before).enumerate(){assert_eq!(*a,*b*scales[i]);}
    let mut scaled=JacobianParts::default();assert!(system.jacobian(0.0,&state,&rate,&mut scaled));
    for (a,b) in scaled.d_dx.iter().chain(&scaled.d_drate).zip(jac.d_dx.iter().chain(&jac.d_drate)) {
        assert_eq!((a.0,a.1),(b.0,b.1));assert_eq!(a.2,b.2*scales[a.0]);
    }
}

#[test]
fn positive_row_scaling_preserves_motion_and_energy_in_physical_units() {
    let mut reference=oscillator();let mut scaled=oscillator();
    let n=scaled.islands[0].system.reduced_dimension();
    scaled.islands[0].system.set_residual_row_scales((0..n).map(|i|if i%2==0 {1e-3}else{1e3}).collect()).unwrap();
    let energy=reference.energy();
    reference.advance(0.5,0.001).unwrap();scaled.advance(0.5,0.001).unwrap();
    for (a,b) in reference.snapshot().islands[0].state.iter().zip(&scaled.snapshot().islands[0].state) {
        assert!((a-b).abs()<1e-9);
    }
    assert!((scaled.energy()-energy).abs()<1e-9);
}
