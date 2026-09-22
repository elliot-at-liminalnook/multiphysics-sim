//! Registry/primitive access only. This executable cannot start an experiment.
use std::io::Read;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = sim_runtime::registry();
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [] => sim_script::primitive_catalogue(&registry),
        [name, version] => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            registry.call_primitive(
                &sim_core::definitions::DefinitionId::new(name, version.parse()?),
                serde_json::from_str(&input)?,
            )?
        }
        _ => return Err("usage: inspect_primitives [NAME VERSION < request.json]".into()),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
