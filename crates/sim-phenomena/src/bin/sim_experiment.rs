use std::{io::Write, path::Path};
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("catalogue") {
        println!(
            "{}",
            sim_script::catalogue(&sim_phenomena::world::registry())
        );
        return;
    }
    if args.first().map(String::as_str) == Some("resolve") && args.len() == 2 {
        let result = std::fs::read_to_string(&args[1])
            .map_err(|e| e.to_string())
            .and_then(|text| {
                serde_json::from_str::<sim_phenomena::experiment::Specification>(&text)
                    .map_err(|e| e.to_string())
            })
            .and_then(|spec| {
                sim_script::evaluate_seeded(
                    &spec.system,
                    &sim_phenomena::world::registry(),
                    sim_script::parameter_map(&spec.parameters).map_err(|e| e.to_string())?,
                    spec.seed,
                )
                .map_err(|e| e.to_string())
            });
        match result {
            Ok(plan) => println!("{}", serde_json::to_string(&plan).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1)
            }
        }
        return;
    }
    if args.len() != 3 || args[0] != "run" {
        eprintln!("usage: sim-experiment run specification.json output-directory | catalogue");
        std::process::exit(2)
    }
    let result = std::fs::read_to_string(&args[1])
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        .and_then(|spec| {
            sim_phenomena::experiment::run(spec, Path::new(&args[2]), |event| {
                println!("{event}");
                let _ = std::io::stdout().flush();
            })
        });
    if let Err(error) = result {
        println!("{}", serde_json::json!({"state":"failed","error":error}));
        eprintln!("{error}");
        std::process::exit(1)
    }
}
