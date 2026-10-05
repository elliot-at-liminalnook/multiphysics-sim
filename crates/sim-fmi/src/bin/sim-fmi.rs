//! `sim-fmi inspect FILE.fmu` prints what an FMU offers (JSON) and whether
//! a block can use it; `sim-fmi pack MODEL_DIR OUT.fmu` builds an FMU from C
//! sources (see `sim_fmi::pack`).
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["inspect", path] => sim_fmi::Fmu::load(path).map_err(|e| e.to_string()).and_then(|fmu| serde_json::to_string_pretty(&fmu.summary()).map_err(|e| e.to_string())),
        ["pack", dir, out] => sim_fmi::pack::pack(std::path::Path::new(dir), std::path::Path::new(out)).map(|sha| format!("{out}\nsha256 {sha}")),
        _ => Err("usage: sim-fmi inspect FILE.fmu | sim-fmi pack MODEL_DIR OUT.fmu".into()),
    };
    match result {
        Ok(text) => println!("{text}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
