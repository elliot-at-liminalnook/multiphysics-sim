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
  study FILE NAME [--threads N]         run a saved comparison or sweep; print the trade-off table
  test FILE [NAME]                     run an acceptance test (keep its evidence beside the file); without
                                       NAME, print where every test stands (not assessed, current, stale)
  datasheet TYPE|--all [--write DIR] [--parameters CAD.physics.json]
                                       run a part's bench; print (or write) its datasheet;
                                       --parameters benches the part as derived from CAD
  realtime FILE [--publish]            measure the realtime profile against the detailed model;
                                       --publish records the measurement in the file
  cad-params FILE AT INSTANCE CAD.physics.json
                                       set an instance's parameters from a CAD derivation (recorded as derived)
  fit SPEC [--promote]                 fit a part to measured data (sim.fit/1); --promote writes the
                                       values as measured (with uncertainty) and publishes the part
  catalog [DATASHEETS_DIR]             Markdown catalog of every annotated part (summary, ports,
                                       datasheet highlights), grouped by palette section
  parts [DIR]                          load authored parts (library/parts); report errors by line
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
    let registry = sim_runtime::system_registry();
    let file = || args.get(1).map(PathBuf::from).ok_or_else(|| USAGE.to_string());
    // The system file's directory: generated robots and FMU blocks resolve against it.
    let dir = || -> Result<PathBuf, String> {
        let f = std::path::absolute(file()?).map_err(|e| e.to_string())?;
        Ok(f.parent().map(Path::to_path_buf).unwrap_or_default())
    };
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
        "test" => {
            let document = store()?.load_valid(&registry).map_err(e)?;
            match args.get(2) {
                Some(name) => {
                    let evidence = sim_runtime::system_evidence::assess(&document, &registry, &dir()?, name, None)?;
                    sim_runtime::system_evidence::save(&file()?, &evidence)?;
                    print(&evidence)?;
                }
                None => print(&sim_runtime::system_evidence::standing(&document, &file()?)?)?,
            }
        }
        "study" => {
            let document = store()?.load_valid(&registry).map_err(e)?;
            let name = args.get(2).ok_or(USAGE)?;
            let study = document.studies.get(name).ok_or_else(|| format!("no study `{name}` (saved: {})", document.studies.keys().cloned().collect::<Vec<_>>().join(", ")))?;
            let threads = flag(&args, "--threads").and_then(|t| t.parse().ok()).unwrap_or(4);
            let result = sim_runtime::system_study::run(&document, &registry, Some(&dir()?), name, study, threads, None, &|done, total| eprintln!("{done}/{total}"))?;
            println!("{}", sim_runtime::system_study::table(&result));
        }
        "datasheet" => {
            let which = args.get(1).ok_or(USAGE)?;
            let types = if which == "--all" { sim_runtime::bench::noted(&registry) } else { vec![which.clone()] };
            let write = flag(&args, "--write").map(PathBuf::from);
            let mut failed = Vec::new();
            let cad = flag(&args, "--parameters").map(|p| sim_runtime::bench::cad_parameters(std::path::Path::new(&p))).transpose()?;
            for t in types {
                let sheet = match &cad {
                    Some((component, params, record)) if *component == t => sim_runtime::bench::datasheet_with(&registry, &t, params, Some(record.clone()))?,
                    Some((component, ..)) => return Err(format!("the CAD derivation is for {component}, not {t}")),
                    None => sim_runtime::bench::datasheet(&registry, &t)?,
                };
                if !sheet.passed() {
                    failed.push(t.clone());
                }
                match &write {
                    Some(dir) => {
                        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                        let path = sim_runtime::bench::path(dir, &t);
                        std::fs::write(&path, serde_json::to_vec_pretty(&sheet).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                        println!("{t}: {} → {}", if sheet.passed() { "pass" } else { "FAIL" }, path.display());
                    }
                    None => print(&sheet)?,
                }
            }
            if !failed.is_empty() {
                return Err(format!("checks failed: {}", failed.join(", ")));
            }
        }
        "realtime" => {
            let store = store()?;
            let document = store.load_valid(&registry).map_err(e)?;
            let report = sim_runtime::realtime_fidelity::measure(&document, &registry)?;
            print(&report.measurement)?;
            let verdict = sim_runtime::realtime_fidelity::within_bound(&document, &report.measurement);
            if args.iter().any(|a| a == "--publish") {
                let mut profile = document.realtime.clone().unwrap();
                profile.measured = Some(report.measurement.clone());
                print(&store.apply(&registry, "Publish realtime measurement", &[sim_system::Command::SetRealtime { realtime: Some(profile) }], Some(document.revision)).map_err(e)?)?;
            }
            verdict?;
        }
        "cad-params" => {
            let (level, instance, physics) = (at(args.get(2).ok_or(USAGE)?), args.get(3).ok_or(USAGE)?.clone(), PathBuf::from(args.get(4).ok_or(USAGE)?));
            let record: serde_json::Value = serde_json::from_slice(&std::fs::read(&physics).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let commands = sim_system::library::cad_physics_commands(&level, &instance, &record).map_err(e)?;
            print(&store()?.apply(&registry, &format!("Parameters from {}", physics.display()), &commands, None).map_err(e)?)?;
        }
        "fit" => {
            use sim_runtime::part_fit::{Condition, Unknown};
            let spec_path = PathBuf::from(args.get(1).ok_or(USAGE)?);
            let spec: serde_json::Value = serde_json::from_slice(&std::fs::read(&spec_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let text = |k: &str| spec[k].as_str().map(str::to_string).ok_or_else(|| format!("fit spec lacks `{k}`"));
            let data_path = spec["data"]["path"].as_str().ok_or("fit spec lacks data.path")?;
            let bytes = std::fs::read(data_path).map_err(|e| format!("{data_path}: {e}"))?;
            let hash = blake3::hash(&bytes).to_hex().to_string();
            if spec["data"]["blake3"].as_str() != Some(hash.as_str()) {
                return Err(format!("{data_path} changed since the spec was written (hash mismatch); rebuild the spec"));
            }
            let model_path = text("model")?;
            let mut model: SystemDocument = serde_json::from_slice(&std::fs::read(&model_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let unknowns: Vec<Unknown> = serde_json::from_value(spec["unknowns"].clone()).map_err(|e| e.to_string())?;
            let conditions: Vec<Condition> = serde_json::from_value(spec["conditions"].clone()).map_err(|e| e.to_string())?;
            let duration = spec["duration"].as_f64().ok_or("fit spec lacks duration")?;
            let result = sim_runtime::part_fit::fit(&model, &registry, &unknowns, &conditions, duration)?;
            for ((name, v), (_, sd)) in result.values.iter().zip(&result.uncertainties) {
                let unit = unknowns.iter().find(|u| &u.name == name).map(|u| u.unit.as_str()).unwrap_or("");
                let compare = spec["compare"][name].as_object().map(|c| format!("   (reference {:.6} ± {:.6}: {})", c["value"].as_f64().unwrap_or(f64::NAN), c["uncertainty"].as_f64().unwrap_or(f64::NAN), c["source"].as_str().unwrap_or(""))).unwrap_or_default();
                println!("{name} = {v:.6} ± {sd:.6} {unit}{compare}");
            }
            for (label, measured, predicted) in &result.residuals {
                println!("  {label}: measured {measured:+.4}, model {predicted:+.4}");
            }
            println!("rms {:.4} after {} iterations", result.rms, result.iterations);
            let mut record = serde_json::json!({"schema": "sim.fit-result/1", "spec": spec_path, "data": {"path": data_path, "blake3": hash}, "result": result});
            if args.iter().any(|a| a == "--promote") {
                let promote: Vec<String> = serde_json::from_value(spec["promote"]["unknowns"].clone()).map_err(|e| e.to_string())?;
                let chosen: Vec<Unknown> = unknowns.iter().filter(|u| promote.contains(&u.name)).cloned().collect();
                let chosen_result = sim_runtime::part_fit::FitResult {
                    values: result.values.iter().filter(|(n, _)| promote.contains(n)).cloned().collect(),
                    uncertainties: result.uncertainties.iter().filter(|(n, _)| promote.contains(n)).cloned().collect(),
                    ..result.clone()
                };
                let commands = sim_runtime::part_fit::promote(&chosen, &chosen_result, data_path, &hash);
                sim_system::apply(&mut model, &registry, &commands).map_err(e)?;
                std::fs::write(&model_path, serde_json::to_vec_pretty(&model).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                let definition = spec["promote"]["definition"].as_str().ok_or("promote.definition")?;
                let library_dir = PathBuf::from(spec["promote"]["library"].as_str().unwrap_or("library/systems"));
                let published = sim_system::library::publish(&model, definition, &library_dir).map_err(e)?;
                for p in &published {
                    println!("published {} v{} ({})", p.id, p.version, if p.changed { "changed" } else { "unchanged" });
                }
                record["published"] = serde_json::json!(published);
            }
            let out = spec_path.with_file_name(spec_path.file_name().unwrap().to_string_lossy().replace(".fit.json", ".fit-result.json"));
            std::fs::write(&out, serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            println!("wrote {}", out.display());
        }
        "catalog" => {
            let sheets = PathBuf::from(args.get(1).cloned().unwrap_or_else(|| "library/datasheets".into()));
            let mut by: std::collections::BTreeMap<String, Vec<String>> = Default::default();
            let order = ["Actuators", "Transmissions", "Mechanical", "Power", "Electrical", "Sensing", "Control", "Thermal"];
            for d in registry.descriptors() {
                let Some(n) = d.notes else { continue };
                let t = &d.type_id.0;
                let ports = sim_system::snap::element_port_types(&registry, t).into_iter().map(|(k, v)| format!("{k} ({v})")).collect::<Vec<_>>().join(", ");
                let sheet: Option<sim_runtime::bench::Datasheet> = std::fs::read(sim_runtime::bench::path(&sheets, t)).ok().and_then(|b| serde_json::from_slice(&b).ok());
                let highlights = sheet.map(|s| s.values.iter().filter(|v| !v.name.contains("audit") && !v.name.contains("realtime")).take(4).map(|v| format!("{} {} {}", v.name, if v.unit == "yes=1" { (if v.value >= 0.5 { "yes" } else { "no" }).to_string() } else { format!("{:.4}", v.value).trim_end_matches('0').trim_end_matches('.').to_string() }, if v.unit == "yes=1" || v.unit == "1" { "" } else { &v.unit })).collect::<Vec<_>>().join("; ")).unwrap_or_default();
                let category = if n.category.is_empty() { "Other".to_string() } else { n.category.to_string() };
                by.entry(category).or_default().push(format!(
                    "### {} — `{t}`\n\n{}\n\n- Ports: {}\n{}{}",
                    d.display_name,
                    n.summary,
                    if ports.is_empty() { "none".into() } else { ports },
                    if n.pairs_with.is_empty() { String::new() } else { format!("- Pairs with: {}\n", n.pairs_with.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ")) },
                    if highlights.is_empty() { String::new() } else { format!("- Datasheet: {highlights}\n") },
                ));
            }
            let mut out = String::from("# Component catalog\n\nEvery annotated part in the registry, grouped by palette section. Generated by `sim-system catalog > library/CATALOG.md`; open a part in the builder's library for its full notes (how it works, equations, trade-offs, limits, parameter help) and datasheet.\n\n");
            let mut sections: Vec<&String> = by.keys().collect();
            sections.sort_by_key(|k| order.iter().position(|o| o == k).unwrap_or(99));
            for k in sections {
                out.push_str(&format!("## {k}\n\n{}\n", by[k].join("\n")));
            }
            print!("{out}");
        }
        "parts" => {
            let dir = args.get(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("library/parts"));
            let mut r = sim_runtime::registry();
            print(&sim_parts::load_dir(&mut r, &dir))?;
        }
        "undo" => print(&store()?.undo().map_err(e)?)?,
        "redo" => print(&store()?.redo().map_err(e)?)?,
        "history" => print(&store()?.history())?,
        "check" => {
            let report = builder::check_at(&store()?.load().map_err(e)?, &registry, &dir()?)?;
            print(&report)?;
            if report.compile_error.is_some() {
                return Err("the system does not compile".into());
            }
        }
        "compile" => {
            let document = store()?.load().map_err(e)?;
            let out = PathBuf::from(args.get(2).ok_or(USAGE)?);
            let stem = flag(&args, "--stem").unwrap_or_else(|| file().unwrap().file_name().unwrap().to_string_lossy().trim_end_matches(".system.json").trim_end_matches(".json").to_string());
            let compiled = builder::compile_at(&document, &registry, builder::config_for(&document), &dir()?)?;
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
            let series = builder::simulate_at(&document, &registry, &dir()?, seconds, config, &flags(&args, "--select"))?;
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
