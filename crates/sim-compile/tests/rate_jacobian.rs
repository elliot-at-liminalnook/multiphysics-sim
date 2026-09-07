use sim_compile::Runtime;
use sim_core::{Behavior,BehaviorDescriptor,BehaviorRegistry,Context,LocalJacobian,ModelWorld,
    QuantityKind,StateDeclaration,View};
use sim_dynamics::{Integrator,JacobianParts,System};
use std::sync::atomic::{AtomicUsize,Ordering};

static EVALUATIONS:AtomicUsize=AtomicUsize::new(0);
struct CoupledRates { exact:bool }
impl Behavior for CoupledRates {
    fn states(&self)->Vec<StateDeclaration> {
        (0..64).map(|i|StateDeclaration::new(format!("x{i}"),QuantityKind::Dimensionless,0.0)).collect()
    }
    fn residual(&self,ctx:&mut Context) {
        EVALUATIONS.fetch_add(1,Ordering::Relaxed);
        for i in 0..64 {
            let x=ctx.state(i);let neighbour=ctx.state((i+1)%64);
            ctx.set_state_residual(i,(1.0+neighbour*neighbour)*ctx.state_rate(i)+x*x*x-neighbour.sin());
        }
    }
    fn rate_jacobian_at(&self,view:&View,_:&[f64],out:&mut LocalJacobian)->bool {
        if !self.exact {return false;}
        for i in 0..64 {out.state_rate(i,i,1.0+view.state((i+1)%64).powi(2));}
        // A shared implementation can emit state entries. They must not replace
        // FD state derivatives, particularly the neighbour-dependent mass term.
        out.state_state(0,0,1234567.0);
        true
    }
}
fn runtime(exact:bool)->Runtime {
    let mut registry=BehaviorRegistry::default();
    registry.register(BehaviorDescriptor::new("test.numeric","Numeric",vec![],
        |_|Ok(Box::new(CoupledRates {exact:false})))).unwrap();
    registry.register(BehaviorDescriptor::new("test.rates","Rates",vec![],
        |_|Ok(Box::new(CoupledRates {exact:true})))).unwrap();
    let mut model=ModelWorld::default();
    model.part(&registry,"coupled",if exact {"test.rates"} else {"test.numeric"},[]).unwrap();
    Runtime::new(model,&registry,Integrator::implicit_midpoint()).unwrap()
}

#[test]
fn exact_rates_skip_probes_and_preserve_state_coupling_in_serial_and_parallel() {
    let baseline=runtime(false);let partial=runtime(true);
    let x:Vec<_>=(0..64).map(|i|0.03*i as f64-0.6).collect();
    let rate:Vec<_>=(0..64).map(|i|(i as f64*0.31).cos()*0.2).collect();
    let evaluate=|system:&sim_compile::island::Island| {
        EVALUATIONS.store(0,Ordering::Relaxed);
        let mut j=JacobianParts::default();
        assert!(system.jacobian(0.2,&x,&rate,&mut j));
        (j,EVALUATIONS.load(Ordering::Relaxed))
    };
    let (reference,count)=evaluate(&baseline.islands[0].system);
    assert_eq!(count,129);
    let (serial,count)=evaluate(&partial.islands[0].system);
    assert_eq!(count,65,"one base plus state probes; no rate perturbations");
    assert_eq!(serial.d_dx,reference.d_dx);
    let (jx,jr)=serial.dense(64);
    for i in 0..64 {
        let next=(i+1)%64;
        assert!((jx[(i,i)]-3.0*x[i]*x[i]).abs()<1e-6);
        assert!((jx[(i,next)]-(2.0*x[next]*rate[i]-x[next].cos())).abs()<1e-6);
        assert_eq!(jr[(i,i)],1.0+x[next]*x[next]);
        for k in 0..64 {if k!=i {assert_eq!(jr[(i,k)],0.0);}}
    }
    #[cfg(all(feature="parallel",not(target_arch="wasm32")))]
    for workers in [1,2,4,8,16] {
        let (parallel,count)=rayon::ThreadPoolBuilder::new().num_threads(workers).build().unwrap()
            .install(||evaluate(&partial.islands[0].system));
        assert_eq!(count,65);
        assert_eq!(parallel.d_dx,serial.d_dx);
        assert_eq!(parallel.d_drate,serial.d_drate);
    }
    let report=sim_dynamics::jacobian_check::check_jacobian(&partial.islands[0].system,
        0.2,&x,&rate,&Default::default()).unwrap();
    assert!(report.passed,"{report:?}");
}
