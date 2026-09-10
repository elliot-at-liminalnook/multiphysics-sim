use sim_runtime::{configuration_inspection::{inspect_configurations, ConfigurationInspection}, session::{Scene, Session}, tracking::CaptureConfig};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len()!=3 { return Err("usage: inspect_configurations scene.json markers.json samples.json".into()); }
    let read = |p:&str| std::fs::read(p).map_err(|e|e.to_string());
    let scene:Scene=serde_json::from_slice(&read(&args[0])?).map_err(|e|e.to_string())?;
    let markers:CaptureConfig=serde_json::from_slice(&read(&args[1])?).map_err(|e|e.to_string())?;
    let config:ConfigurationInspection=serde_json::from_slice(&read(&args[2])?).map_err(|e|e.to_string())?;
    let session=Session::new(scene,0)?;
    let start=std::time::Instant::now();
    let rows=inspect_configurations(&session.robot.art,&session.robot.generalized(),&markers,&config)?;
    println!("{}",serde_json::to_string(&serde_json::json!({"rows":rows,"inspection_wall_s":start.elapsed().as_secs_f64(),"link_names":session.robot.art.links.iter().map(|l|&l.name).collect::<Vec<_>>(),"coordinate_frame":markers.coordinate_frame,"cad_sha256":markers.expected_cad_sha256})).map_err(|e|e.to_string())?);
    Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("{e}");std::process::exit(1)}}
