//! Thin durable host for the shared reduced-exploration library. No optimizer.
use serde::{Serialize, de::DeserializeOwned};
use sim_runtime::{
    experiment::ExperimentSpec,
    exploration::{self, CaptureSession, Profile, Recipe},
    fidelity::{EnvironmentCapture, Provenance},
};
use std::{
    fs::{self, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::Path,
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn read<T: DeserializeOwned>(p: impl AsRef<Path>) -> Result<T> {
    Ok(serde_json::from_reader(BufReader::new(fs::File::open(p)?))?)
}
fn write(p: impl AsRef<Path>, v: &impl Serialize) -> Result<()> {
    let file = OpenOptions::new().write(true).create_new(true).open(p)?;
    let mut out = BufWriter::new(file);
    serde_json::to_writer(&mut out, v)?;
    out.flush()?;
    out.get_ref().sync_all()?;
    Ok(())
}
fn provenance(root: &Path, name: &str) -> Result<Provenance> {
    let executable = std::env::current_exe()?;
    Ok(Provenance{capture_reference:root.join(format!("{name}.capture.json")).display().to_string(),
        source_reference:serde_json::to_string(&sim_runtime::physics_context::RuntimeIdentity::current())?,
        executable_reference:format!("{} blake3:{}",executable.display(),blake3::hash(&fs::read(&executable)?)),
        host_reference:format!("{} {} {}",std::env::consts::OS,std::env::consts::ARCH,std::env::var("HOSTNAME").unwrap_or_else(|_|"local host".into())),
        timing_scope:"Sequential unprofiled task stepping, observations and in-memory capture; excludes construction and final serialization".into()})
}
fn run(
    root: &Path,
    name: &str,
    spec: &ExperimentSpec,
    cancel: Option<&String>,
) -> Result<EnvironmentCapture> {
    if root.join(format!("{name}.capture.json")).exists() {
        return Err("capture already exists; use compare or a fresh output directory".into());
    }
    eprintln!("{name}: constructing production environment");
    let mut session = CaptureSession::new(spec)?;
    let start = Instant::now();
    let mut reported = -1.;
    while !session.done() && !cancel.is_some_and(|p| Path::new(p).exists()) {
        session.advance()?;
        if session.time_s() - reported >= 0.5 {
            eprintln!(
                "{name}: {:.2}s simulated, {:.2}s wall",
                session.time_s(),
                start.elapsed().as_secs_f64()
            );
            reported = session.time_s();
        }
    }
    let capture = session.capture(start.elapsed().as_secs_f64());
    write(root.join(format!("{name}.capture.json")), &capture)?;
    Ok(capture)
}
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str){
        Some("prepare") if (5..=6).contains(&args.len())=>{
            let mut detailed:ExperimentSpec=read(&args[1])?;let profile:Profile=read(&args[2])?;
            // Optional explicit prefix is recorded in the derived recipe only.
            if let Some(seconds)=args.get(5){
                let seconds:f64=seconds.parse()?;let count=seconds/detailed.task.period_s;
                if !seconds.is_finite()||seconds<=0.||(count-count.round()).abs()>1e-8||count>detailed.source_actions.len()as f64{return Err("prefix must be a positive aligned duration within the source schedule".into());}
                let steps=seconds/detailed.config.step_s;
                if (steps-steps.round()).abs()>1e-8{return Err("prefix must align with physics steps".into());}
                detailed.config.steps=steps.round()as usize;detailed.scene.duration_s=seconds;detailed.source_actions.truncate(count.round()as usize);
            }
            let recipe=Recipe{version:1,detailed,profile};let prepared=recipe.prepare()?;
            let root=Path::new(&args[3]);
            if args[4]!="--fresh"{return Err("prepare requires --fresh to make output semantics explicit".into());}
            fs::create_dir(root)?;
            write(root.join("recipe.json"),&recipe)?;write(root.join("prepared.json"),&prepared)?;
            write(root.join("detailed.spec.json"),&prepared.detailed)?;write(root.join("reduced.spec.json"),&prepared.reduced)?;
            write(root.join("source.json"),&serde_json::json!({"source_spec":args[1],"source_blake3":blake3::hash(&fs::read(&args[1])?).to_hex().to_string(),"prefix_s":args.get(5),"status":"prepared_only_not_qualified"}))?;
            println!("Prepared {} (no simulation/search)",root.display());
        },
        Some(command @ ("qualify"|"compare")) if (2..=3).contains(&args.len())=>{
            let root=Path::new(&args[1]);
            let lock=OpenOptions::new().create(true).append(true).open(root.join("qualification.lock"))?;
            lock.try_lock().map_err(|e|format!("qualification directory already has a writer: {e}"))?;
            let recipe:Recipe=read(root.join("recipe.json"))?;let prepared=recipe.prepare()?;
            let (a,b)=if command=="qualify"{
                // A detailed reference already captured for this same recipe (e.g. copied
                // from another profile's qualification of the same baseline) is reused;
                // qualify still checks it against this recipe and runtime.
                let a=if root.join("detailed.capture.json").exists(){read(root.join("detailed.capture.json"))?}else{run(root,"detailed",&prepared.detailed,args.get(2))?};
                if args.get(2).is_some_and(|p|Path::new(p).exists()){return Err("cancelled; retained detailed prefix; no qualification".into());}
                (a,run(root,"reduced",&prepared.reduced,args.get(2))?)
            }else{(read(root.join("detailed.capture.json"))?,read(root.join("reduced.capture.json"))?)};
            let result=exploration::qualify(&recipe,&a,&b,provenance(root,"detailed")?,provenance(root,"reduced")?);
            match result{
                Ok(q)=>{write(root.join("qualification.json"),&q)?;println!("{}",serde_json::json!({"qualified":q.qualified,"speedup":q.speedup,"reasons":q.rejection_reasons}));if !q.qualified{return Err("profile failed its declared qualification gates; captures retained".into());}},
                Err(e)=>{write(root.join("qualification.error.json"),&serde_json::json!({"qualified":false,"error":e}))?;return Err(e.into());}
            }
        },
        Some("finalist") if args.len()==4=>{
            let recipe:Recipe=read(Path::new(&args[1]).join("recipe.json"))?;
            let detailed=exploration::detailed_candidate(&recipe,read(&args[2])?)?;
            write(&args[3],&detailed)?;println!("Detailed finalist prepared; not run");
        },
        _=>return Err("usage: reduced_exploration prepare spec.json profile.json fresh-directory --fresh [prefix-seconds]; qualify directory [cancel-file]; compare directory; finalist directory values.json fresh-spec.json".into()),
    }
    Ok(())
}
