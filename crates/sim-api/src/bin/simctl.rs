//! Invoke ordered batches across the local Rust viewers in one command.
use serde_json::Value;
fn run() -> Result<Value, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag, file] if flag == "--plan" => {
            let plan = serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            sim_api::client::plan(&plan, std::time::Duration::from_secs(300))
        }
        [flag, url, path] if flag == "get" => sim_api::client::request(url, "GET", path, None),
        [url, file] => {
            let batch = serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            sim_api::client::batch(url, &batch, std::time::Duration::from_secs(300))
        }
        _ => Err(
            "usage: simctl URL batch.json | simctl --plan plan.json | simctl get URL /v1/state"
                .into(),
        ),
    }
}
fn main() {
    match run() {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
            let failed = value
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|s| matches!(s, "failed" | "cancelled"))
                || value
                    .get("results")
                    .and_then(Value::as_array)
                    .is_some_and(|rs| {
                        rs.iter().any(|r| {
                            r.get("error").is_some()
                                || r.get("job").is_some_and(|j| j["status"] != "succeeded")
                        })
                    });
            if failed {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
