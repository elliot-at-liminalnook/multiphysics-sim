use sim_domain_control::{neural::*, ppo::*};
use sim_core::QuantityKind as Q;
fn network()->Network {
    Network{version:1,features:vec![Feature{source:"x".into(),subtract:None,kind:Q::Dimensionless,center:0.,scale:1.,clip:10.}],
        outputs:vec![Output{target:"y".into(),kind:Q::Dimensionless,scale:1.}],
        layers:vec![Layer{weights:vec![vec![0.4],vec![-0.2]],biases:vec![0.1,0.3]},Layer{weights:vec![vec![0.2,-0.3]],biases:vec![0.1]}]}
}
#[test]
fn ppo_derivative_matches_finite_differences_including_both_clipped_branches(){
    let n=network();let config=GaussianExploration{standard_deviation:vec![0.4]};
    for (advantage,ratio) in [(1.,1.1),(1.,1.5),(-1.,0.5),(-1.,1.1)] {
        let x=vec![0.6];let a=vec![0.25];let logp=config.log_probability(&n.normalized_output(&x,false).unwrap(),&a).unwrap();
        let s=PolicySample{inputs:x,raw_actions:a,old_log_probability:logp-f64::ln(ratio),advantage};
        let (report,g)=policy_gradient(&n,&config,&[s.clone()],0.2).unwrap();
        for i in 0..g.len(){let mut p=n.parameters();let mut m=p.clone();p[i]+=1e-6;m[i]-=1e-6;
            let delta=(policy_gradient(&n.with_parameters(&p).unwrap(),&config,&[s.clone()],0.2).unwrap().0.loss
                -policy_gradient(&n.with_parameters(&m).unwrap(),&config,&[s.clone()],0.2).unwrap().0.loss)/2e-6;
            assert!((g[i]-delta).abs()<1e-8);
        }
        if ratio==1.5||ratio==0.5 {assert_eq!(report.clipped_fraction,1.);assert!(g.iter().all(|v|*v==0.));}
    }
}
#[test]
fn seeded_gaussian_replays_and_has_expected_moments(){
    let c=GaussianExploration{standard_deviation:vec![2.]};
    let mut a=GaussianSampler::new(c.clone(),1,42).unwrap();let mut b=GaussianSampler::new(c.clone(),1,42).unwrap();
    let mut sum=0.;let mut squares=0.;
    for _ in 0..20_000{let x=a.sample(&[3.]).unwrap();assert_eq!(x,b.sample(&[3.]).unwrap());sum+=x.0[0];squares+=(x.0[0]-3.).powi(2);}
    assert!((sum/20_000.-3.).abs()<0.04);assert!((squares/20_000.-4.).abs()<0.12);
    assert!(GaussianSampler::new(GaussianExploration{standard_deviation:vec![0.]},1,0).is_err());
}
#[test]
fn shuffle_is_a_reproducible_permutation_and_preserves_exploration_sequence(){
    use sim_domain_control::optimization::seeded_permutation;
    for n in [0,1,17,256]{let p=seeded_permutation(n,73);assert_eq!(p,seeded_permutation(n,73));let mut sorted=p;sorted.sort();assert_eq!(sorted,(0..n).collect::<Vec<_>>());}
    assert_ne!(seeded_permutation(256,73),seeded_permutation(256,74));
    let mut counts=[[0;5];5];for seed in 0..20_000{for(i,v)in seeded_permutation(5,seed).iter().enumerate(){counts[i][*v]+=1;}}
    assert!(counts.iter().flatten().all(|n|(*n as f64-4000.).abs()<300.));
    // Golden first draw from the archived pre-refactor full-robot seed 74.
    let mut rng=GaussianSampler::new(GaussianExploration{standard_deviation:vec![0.01;12]},12,74^0x504f4c494359).unwrap();
    assert_eq!(rng.sample(&[0.;12]).unwrap().0,vec![-0.0031541311642181782,-0.004936166630754181,0.01172933049293428,-0.00022203811016790624,-0.014176966031063581,0.009162746852109644,0.0052538349763094775,0.00019118097367881726,0.026847723487623633,-0.011450535755436931,0.0226503906394271,0.010736500286219064]);
}
#[test]
fn exact_gaussian_kl_matches_likelihood_expectation(){
    let c=GaussianExploration{standard_deviation:vec![2.,0.5]};let old=[0.,2.];let new=[1.,3.];
    assert_eq!(c.kl(&old,&new).unwrap(),2.125);assert_eq!(c.kl(&old,&old).unwrap(),0.);
    let mut rng=GaussianSampler::new(c.clone(),2,11).unwrap();let mut total=0.;
    for _ in 0..20_000{let(a,logp)=rng.sample(&old).unwrap();total+=logp-c.log_probability(&new,&a).unwrap();}
    assert!((total/20_000.-c.kl(&old,&new).unwrap()).abs()<0.06);
    assert!(c.kl(&old,&[1.]).is_err());assert!(c.kl(&old,&[1.,f64::NAN]).is_err());
}
#[test]
fn linear_critic_gradient_and_finite_horizon_returns(){
    let n=network();let sample=ValueSample{inputs:vec![0.7],targets:vec![12.]};
    let (_,g)=value_gradient(&n,&[sample.clone()]).unwrap();
    for i in 0..g.len(){let mut p=n.parameters();let mut m=p.clone();p[i]+=1e-6;m[i]-=1e-6;
        let d=(value_gradient(&n.with_parameters(&p).unwrap(),&[sample.clone()]).unwrap().0-value_gradient(&n.with_parameters(&m).unwrap(),&[sample.clone()]).unwrap().0)/2e-6;
        assert!((g[i]-d).abs()<1e-7);
    }
    let a=advantages(&[1.,-0.5,2.],&[0.3,0.2,0.1],0.,1.,1.).unwrap();
    for (actual,expected) in a.iter().zip([2.2,1.3,1.9]){assert!((actual-expected).abs()<1e-12);}
    assert_eq!(advantages(&[1.],&[0.5],2.,0.9,1.).unwrap(),vec![2.3]);
}
#[test]
fn ppo_improves_a_known_bandit_with_actions_from_its_own_distribution(){
    let mut n=network();let config=GaussianExploration{standard_deviation:vec![0.3]};
    let mut rng=GaussianSampler::new(config.clone(),1,12).unwrap();let mut adam=Adam::new(n.parameters().len());
    let initial=n.normalized_output(&[0.],false).unwrap()[0];
    for _ in 0..20{
        let mean=n.normalized_output(&[0.],false).unwrap();
        let mut data=vec![];
        for _ in 0..128{let(a,logp)=rng.sample(&mean).unwrap();let advantage=-(a[0]-0.7).powi(2);
            data.push(PolicySample{inputs:vec![0.],raw_actions:a,old_log_probability:logp,advantage});}
        let average=data.iter().map(|s|s.advantage).sum::<f64>()/data.len() as f64;
        for s in &mut data{s.advantage-=average;}
        for _ in 0..4{let(_,g)=policy_gradient(&n,&config,&data,0.2).unwrap();n=n.with_parameters(&adam.step(&n.parameters(),&g,0.01,Some(1.)).unwrap()).unwrap();}
    }
    let final_mean=n.normalized_output(&[0.],false).unwrap()[0];
    assert!((final_mean-0.7).abs()<0.12,"initial {initial}, final {final_mean}");
    assert!((final_mean-0.7).abs()<(initial-0.7).abs());
}
