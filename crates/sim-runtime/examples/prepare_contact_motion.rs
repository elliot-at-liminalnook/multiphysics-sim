//! File adapter: expensive preparation stays off the UI thread.
use sim_runtime::contact_exploration::Recipe;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: prepare_contact_motion recipe.json values.json fresh-directory".into());
    }
    let mut recipe: Recipe = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    recipe.sync_actuators(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
    let values = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let start = std::time::Instant::now();
    let result = recipe.prepare(&values);
    std::fs::create_dir(&args[2])?;
    let save = |name: &str, value: serde_json::Value| -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(std::path::Path::new(&args[2]).join(name))?;
        let mut output = std::io::BufWriter::new(file);
        serde_json::to_writer(&mut output, &value)?;
        output.flush()?;
        Ok(())
    };
    save("values.json", serde_json::to_value(&values)?)?;
    save(
        "timing.json",
        serde_json::json!({"preparation_wall_s":start.elapsed().as_secs_f64(),"simulation_run":false}),
    )?;
    match result {
        Ok(p) => {
            save("spec.json", serde_json::to_value(p.spec)?)?;
            save("compiled.json", p.compiled)?;
            save("screen.json", p.screen)?;
            println!("Prepared and screened; no simulation run");
        }
        Err(e) => {
            save("rejection.json", serde_json::json!({"error":e}))?;
            return Err(e.into());
        }
    }
    Ok(())
}
