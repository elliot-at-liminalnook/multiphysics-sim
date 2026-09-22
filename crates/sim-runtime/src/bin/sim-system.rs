//! Build, edit, check and run hierarchical system files from the shell.
//! Every edit goes through the same commands, validation and shared undo
//! journal as the viewers and their REST APIs.
use sim_runtime::system_builder as builder;
use sim_system::{library, Command, ReferenceView, SystemDocument, SystemStore};
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: sim-system <command> …
  new FILE [--title TEXT]              create an empty system file
  show FILE [--at PATH]                list one level: instances, ports, nets
  do FILE JSON [--label TEXT] [--expect REVISION]
                                       apply one command object or an array of them
  apply FILE COMMANDS.json|- […]       same, reading JSON from a file or stdin
  undo FILE | redo FILE | history FILE shared history with the viewers
  check FILE                           validate, list findings, construct the runtime
  compile FILE OUT_DIR [--stem NAME]   write description, spatial, animation, live capture
  run FILE SECONDS [--interval S] [--abs TOL] [--rel TOL] [--select TEXT]… [--csv OUT]
                                       simulate headlessly; print final/min/max per series
  elements [--domain NAME]             the element palette (JSON)
  library list [DIR] | library save FILE DEFINITION [DIR] | library import FILE LIBRARY_FILE
  alternatives FILE AT NAME [--library DIR]
                                       implementations that can replace an instance
  reference FILE AT ID IMAGE [--view spatial|schematic] [--width W] [--origin X,Y,Z]
                                       import a PNG/JPEG reference image
Paths: AT is an instance path from the root, `` or `/` for the root itself.
The default library directory is library/systems (override with SIM_SYSTEM_LIBRARY).";

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}
fn flags(args: &[String], name: &str) -> Vec<String> {
    args.windows(2).filter(|w| w[0] == name).map(|w| w[1].clone()).collect()
}
fn at(path: &str) -> String {
    path.trim_matches('/').to_string()
}
fn default_library() -> PathBuf {
    std::env::var_os("SIM_SYSTEM_LIBRARY").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("library/systems"))
}
fn print<T: serde::Serialize>(value: &T) -> Result<(), String> {
    println!("{}", serde_json::to_string_pretty(value).map_err(|e| e.to_string())?);
    Ok(())
}
fn parse_commands(text: &str) -> Result<Vec<Command>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("commands are not JSON: {e}"))?;
    match value {
        serde_json::Value::Array(items) => items.into_iter().map(|v| serde_json::from_value(v).map_err(|e| e.to_string())).collect(),
        v => Ok(vec![serde_json::from_value(v).map_err(|e| e.to_string())?]),
    }
}

fn show(document: &SystemDocument, registry: &sim_core::BehaviorRegistry, path: &str) -> Result<(), String> {
    let resolver = sim_system::Resolver::new(document, registry);
    let id = resolver.definition_id_at(path).map_err(|e| e.to_string())?;
    let d = resolver.definition(&id).map_err(|e| e.to_string())?;
    println!("{} — revision {} — level `{}` = definition `{id}` ({}), placed {}×", document.title, document.revision, if path.is_empty() { "/" } else { path }, d.label, resolver.placements(&id));
    if !d.parameters.is_empty() {
        println!("parameters:");
        for (name, p) in &d.parameters {
            println!("  {name} [{}] default {:?} {}", p.unit, p.default, p.description);
        }
    }
    if !d.ports.is_empty() {
        println!("boundary ports:");
        for name in d.ports.keys() {
            let schema = resolver.boundary_schema(&id, name, &mut Default::default()).map_err(|e| e.to_string())?;
            println!("  {name}: {}", schema.as_ref().map(sim_system::commands::describe).unwrap_or_else(|| "untyped".into()));
        }
    }
    println!("instances:");
    for (name, i) in &d.instances {
        let params: Vec<String> = i
            .parameters
            .iter()
            .map(|(k, v)| match v {
                sim_system::ParameterBinding::Value { value, .. } => format!("{k}={value}"),
                sim_system::ParameterBinding::Parameter { parameter } => format!("{k}=${parameter}"),
            })
            .collect();
        println!("  {name}: {} {}{}", sim_system::commands::kind_label(&i.kind), if i.label.is_empty() { String::new() } else { format!("\"{}\" ", i.label) }, params.join(" "));
    }
    println!("nets:");
    for net in &d.nets {
        println!("  {}{}", if net.label.is_empty() { String::new() } else { format!("{}: ", net.label) }, net.terminals.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(" — "));
    }
    if !d.references.is_empty() {
        println!("reference images: {}", d.references.keys().cloned().collect::<Vec<_>>().join(", "));
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().ok_or(USAGE)?.as_str();
    let registry = sim_runtime::registry();
    let file = || args.get(1).map(PathBuf::from).ok_or_else(|| USAGE.to_string());
    let store = || file().map(SystemStore::new);
    let e = |e: sim_system::SystemError| e.to_string();
    match command {
        "new" => {
            let document = SystemDocument::new(&flag(&args, "--title").unwrap_or_else(|| "New system".into()));
            SystemStore::create(file()?, &document).map_err(e)?;
            println!("created {}", file()?.display());
        }
        "show" => show(&store()?.load_valid(&registry).map_err(e)?, &registry, &at(&flag(&args, "--at").unwrap_or_default()))?,
        "do" | "apply" => {
            let source = args.get(2).ok_or(USAGE)?;
            let text = if command == "do" {
                source.clone()
            } else if source == "-" {
                std::io::read_to_string(std::io::stdin()).map_err(|e| e.to_string())?
            } else {
                std::fs::read_to_string(source).map_err(|e| e.to_string())?
            };
            let commands = parse_commands(&text)?;
            let expected = flag(&args, "--expect").map(|v| v.parse::<u64>().map_err(|_| "invalid --expect")).transpose()?;
            let label = flag(&args, "--label").unwrap_or_else(|| format!("{} command(s) from CLI", commands.len()));
            print(&store()?.apply(&registry, &label, &commands, expected).map_err(e)?)?;
        }
        "undo" => print(&store()?.undo().map_err(e)?)?,
        "redo" => print(&store()?.redo().map_err(e)?)?,
        "history" => print(&store()?.history())?,
        "check" => {
            let report = builder::check(&store()?.load().map_err(e)?, &registry)?;
            print(&report)?;
            if report.compile_error.is_some() {
                return Err("the system does not compile".into());
            }
        }
        "compile" => {
            let document = store()?.load().map_err(e)?;
            let out = PathBuf::from(args.get(2).ok_or(USAGE)?);
            let stem = flag(&args, "--stem").unwrap_or_else(|| file().unwrap().file_name().unwrap().to_string_lossy().trim_end_matches(".system.json").trim_end_matches(".json").to_string());
            let compiled = builder::compile(&document, &registry, builder::config_for(&document))?;
            let bundle = builder::write_bundle(&compiled, &out, &stem)?;
            println!("description {}\nlive {}", bundle.description.display(), bundle.live.display());
            if let Some(s) = bundle.spatial {
                println!("spatial {}", s.display());
            }
            if let Some(a) = bundle.animation {
                println!("animation {}", a.display());
            }
        }
        "run" => {
            let document = store()?.load().map_err(e)?;
            let seconds: f64 = args.get(2).ok_or(USAGE)?.parse().map_err(|_| "SECONDS must be a number")?;
            let mut config = builder::config_for(&document);
            if let Some(i) = flag(&args, "--interval") {
                config.interval = i.parse().map_err(|_| "invalid --interval")?;
            }
            match flag(&args, "--integrator").as_deref() {
                None => {}
                Some("midpoint") => {
                    if let sim_dynamics::Integrator::BackwardEuler(n) = config.integrator {
                        config.integrator = sim_dynamics::Integrator::ImplicitMidpoint(n);
                    }
                }
                Some("euler") => {
                    if let sim_dynamics::Integrator::ImplicitMidpoint(n) = config.integrator {
                        config.integrator = sim_dynamics::Integrator::BackwardEuler(n);
                    }
                }
                Some(other) => return Err(format!("unknown integrator `{other}` (midpoint or euler)")),
            }
            if let sim_dynamics::Integrator::ImplicitMidpoint(n) | sim_dynamics::Integrator::BackwardEuler(n) = &mut config.integrator {
                if let Some(v) = flag(&args, "--abs") {
                    n.absolute_tolerance = v.parse().map_err(|_| "invalid --abs")?;
                }
                if let Some(v) = flag(&args, "--rel") {
                    n.relative_tolerance = v.parse().map_err(|_| "invalid --rel")?;
                }
            }
            let started = std::time::Instant::now();
            let series = builder::simulate(&document, &registry, seconds, config, &flags(&args, "--select"))?;
            let wall = started.elapsed().as_secs_f64();
            for s in &series {
                let (min, max) = s.values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
                println!("{:<60} final {:>12.6} {:<6} min {:>12.6} max {:>12.6}", s.label, s.values.last().copied().unwrap_or(f64::NAN), s.unit, min, max);
            }
            println!("simulated {seconds} s in {wall:.3} s wall ({:.2}× real time)", seconds / wall);
            if let Some(csv) = flag(&args, "--csv") {
                // Series sample at different instants (step ends or accepted
                // stages); rows are the union of sample times, blank where absent.
                let mut rows: std::collections::BTreeMap<u64, Vec<Option<f64>>> = Default::default();
                for (k, s) in series.iter().enumerate() {
                    for (t, v) in s.times.iter().zip(&s.values) {
                        rows.entry(t.to_bits()).or_insert_with(|| vec![None; series.len()])[k] = Some(*v);
                    }
                }
                let mut text = String::from("time");
                for s in &series {
                    text += &format!(",\"{} [{}]\"", s.label.replace('"', "'"), s.unit);
                }
                text.push('\n');
                for (t, values) in rows {
                    text += &format!("{}", f64::from_bits(t));
                    for v in values {
                        text += &v.map(|v| format!(",{v}")).unwrap_or_else(|| ",".into());
                    }
                    text.push('\n');
                }
                std::fs::write(&csv, text).map_err(|e| e.to_string())?;
                println!("wrote {csv}");
            }
        }
        "elements" => {
            let domain = flag(&args, "--domain");
            let entries: Vec<_> = library::elements(&registry).into_iter().filter(|x| domain.as_ref().is_none_or(|d| &x.domain == d)).collect();
            print(&entries)?;
        }
        "library" => match args.get(1).map(String::as_str) {
            Some("list") => print(&library::list(&args.get(2).map(PathBuf::from).unwrap_or_else(default_library), &registry).map_err(e)?)?,
            Some("save") => {
                let document = SystemStore::new(args.get(2).ok_or(USAGE)?).load().map_err(e)?;
                let definition = args.get(3).ok_or(USAGE)?;
                let path = library::save(&document, definition, &args.get(4).map(PathBuf::from).unwrap_or_else(default_library)).map_err(e)?;
                println!("saved {}", path.display());
            }
            Some("import") => {
                let target = SystemStore::new(args.get(2).ok_or(USAGE)?);
                let source = Path::new(args.get(3).ok_or(USAGE)?);
                let definitions = library::import(source).map_err(e)?;
                print(&target.apply(&registry, &format!("Import {}", source.display()), &[Command::AddDefinitions { definitions }], None).map_err(e)?)?;
            }
            _ => return Err(USAGE.into()),
        },
        "alternatives" => {
            let document = store()?.load().map_err(e)?;
            let library_dir = flag(&args, "--library").map(PathBuf::from).unwrap_or_else(default_library);
            print(&library::alternatives(&document, &registry, Some(&library_dir), &at(args.get(2).ok_or(USAGE)?), args.get(3).ok_or(USAGE)?).map_err(e)?)?;
        }
        "reference" => {
            let view = match flag(&args, "--view").as_deref() {
                None | Some("spatial") => ReferenceView::Spatial,
                Some("schematic") => ReferenceView::Schematic,
                Some(other) => return Err(format!("unknown view `{other}`")),
            };
            let width: f32 = flag(&args, "--width").map(|w| w.parse().map_err(|_| "invalid --width")).transpose()?.unwrap_or(if view == ReferenceView::Spatial { 0.1 } else { 400. });
            let origin = match flag(&args, "--origin") {
                Some(text) => {
                    let v: Vec<f32> = text.split(',').map(|x| x.trim().parse().map_err(|_| "invalid --origin")).collect::<Result<_, _>>()?;
                    <[f32; 3]>::try_from(v).map_err(|_| "--origin needs X,Y,Z")?
                }
                None => [0.; 3],
            };
            let applied = store()?
                .import_reference(&registry, &at(args.get(2).ok_or(USAGE)?), args.get(3).ok_or(USAGE)?, Path::new(args.get(4).ok_or(USAGE)?), view, origin, width)
                .map_err(e)?;
            print(&applied)?;
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
