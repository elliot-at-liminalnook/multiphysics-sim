//! Compare actuator-driven motion to its prescribed geometric reference.
fn main() {
    let run = || -> Result<serde_json::Value, String> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.len() != 4 {
            return Err(
                "usage: compare_motion simulation.json sweep.json markers.json reference-link"
                    .into(),
            );
        }
        let read = |p: &str| -> Result<serde_json::Value, String> {
            serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        };
        sim_runtime::motion_tracking::compare_motion(
            &read(&args[0])?,
            &read(&args[1])?,
            &serde_json::from_value(read(&args[2])?).map_err(|e| e.to_string())?,
            &args[3],
        )
    };
    match run() {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
