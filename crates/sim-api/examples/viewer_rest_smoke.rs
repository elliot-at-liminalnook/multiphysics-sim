//! End-to-end REST acceptance against the actual headless viewer executables.
//! Run after building sim-spatial, sim-viewer and sim-system-worker.
use serde_json::{Value, json};
use sim_api::client::{batch, plan, request};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(
            start.elapsed() < Duration::from_secs(45),
            "timed out waiting for headless host"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}
fn spawn(root: &Path, temp: &Path, name: &str, args: &[String]) -> (Process, String) {
    let log = temp.join(format!("{name}.log"));
    let file = std::fs::File::create(&log).unwrap();
    let mut child = Process(
        Command::new(
            std::env::var_os("SIM_REST_BINARY_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("target/debug"))
                .join(name),
        )
        .args(args)
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(file)
        .spawn()
        .unwrap(),
    );
    let mut url = String::new();
    wait(|| {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "{name} exited: {}",
            std::fs::read_to_string(&log).unwrap()
        );
        if let Some(line) = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .find(|l| l.contains("REST: http://"))
        {
            url = line.split("REST: ").nth(1).unwrap().trim().into();
            true
        } else {
            false
        }
    });
    wait(|| request(&url, "GET", "/v1/state", None).is_ok());
    (child, url)
}
fn call(url: &str, commands: Value) -> Value {
    let job = batch(url, &json!({"commands":commands}), Duration::from_secs(45)).unwrap();
    assert_eq!(job["status"], "succeeded", "{job}");
    job
}
fn op(name: &str, args: Value) -> Value {
    json!({"command":name,"args":args})
}
fn experiment(url: &str, action: Value) -> Value {
    call(url, json!([op("experiments", json!({"action":action}))]))["results"][0]["value"].clone()
}
fn note(url: &str, action: Value) -> Value {
    call(url, json!([op("annotations", json!({"action":action}))]))["results"][0]["value"].clone()
}
fn render(url: &str, args: Value, evidence: &Path, name: &str) -> Value {
    let job = call(url, json!([op("render", args)]));
    let artifact = &job["results"][0]["value"];
    let bytes = sim_api::client::image(url, artifact["url"].as_str().unwrap()).unwrap();
    assert!(bytes.len() > 1024);
    assert_eq!(&bytes[12..16], b"IHDR");
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 1280);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 900);
    let metadata = request(url, "GET", artifact["metadata_url"].as_str().unwrap(), None).unwrap();
    std::fs::write(evidence.join(format!("{name}.png")), bytes).unwrap();
    std::fs::write(
        evidence.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&metadata).unwrap(),
    )
    .unwrap();
    job
}
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let evidence = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("viewer-rest-{}", std::process::id()))
        });
    std::fs::create_dir_all(&evidence).unwrap();
    let evidence = evidence.canonicalize().unwrap();
    let source = root.join("examples/systems-viewer/spatial/motor-thermal.description.json");
    let retained = std::fs::read(&source).unwrap();
    let description: Value = serde_json::from_slice(&retained).unwrap();
    let link = evidence.join("selection-link");
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&link)
        .unwrap();
    std::fs::write(link.join("selection.json"),serde_json::to_vec(&json!({"version":1,"description_id":description["id"],"sequence":0,"origin":"assembly","target":{"kind":"none"}})).unwrap()).unwrap();
    let animation_path = root.join("examples/systems-viewer/spatial/motor-thermal.animation.json");
    let spatial_path = root.join("examples/systems-viewer/spatial/motor-thermal.spatial.json");
    let retained_inputs: Vec<_> = [
        source.clone(),
        spatial_path.clone(),
        animation_path.clone(),
        root.join("examples/systems-viewer/spatial/motor-thermal.live.json"),
    ]
    .into_iter()
    .map(|p| {
        let bytes = std::fs::read(&p).unwrap();
        (p, bytes)
    })
    .collect();

    let annotations = evidence.join("discussion.annotations.json");
    let (_physical, physical) = spawn(
        &root,
        &evidence,
        "sim-spatial",
        &[
            "--headless".into(),
            "--annotations".into(),
            annotations.display().to_string(),
            "--api-port".into(),
            "0".into(),
            "--selection-link".into(),
            link.display().to_string(),
            "--animation".into(),
            animation_path.display().to_string(),
        ],
    );
    let workspace = evidence.join("review.workspace.json");
    let (_schematic, schematic) = spawn(
        &root,
        &evidence,
        "sim-viewer",
        &[
            "--headless".into(),
            "--annotations".into(),
            annotations.display().to_string(),
            "--api-port".into(),
            "0".into(),
            "--description".into(),
            source.display().to_string(),
            "--live".into(),
            root.join("examples/systems-viewer/spatial/motor-thermal.live.json")
                .display()
                .to_string(),
            "--workspace".into(),
            workspace.display().to_string(),
            "--selection-link".into(),
            link.display().to_string(),
            "--animation".into(),
            animation_path.display().to_string(),
            "--spatial".into(),
            spatial_path.display().to_string(),
        ],
    );
    let description: Value = serde_json::from_slice(&retained).unwrap();
    let id = description["components"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    wait(|| {
        let s = request(&schematic, "GET", "/v1/state", None).unwrap();
        s["live"]["status"]["phase"] == "paused" && s["layout_pending"] == false
    });
    let mut receipts = Vec::new();
    receipts.push(plan(&json!({"batches":[{"url":physical,"commands":[op("select",json!({"target":{"kind":"components","ids":[id]}})),op("display",json!({"action":{"kind":"hide_selected"}})),op("display",json!({"action":{"kind":"set_exploded","enabled":true}})),op("state",json!({}))]},{"url":schematic,"commands":[op("select",json!({"target":{"kind":"components","ids":[id]}})),op("state",json!({}))]}]}),Duration::from_secs(30)).unwrap());
    wait(|| request(&physical, "GET", "/v1/state", None).unwrap()["display"]["exploded"] == true);
    let state = request(&physical, "GET", "/v1/state", None).unwrap();
    assert_eq!(state["display"]["exploded"], true);
    assert!(
        state["display"]["hidden"]
            .as_array()
            .unwrap()
            .contains(&json!(id))
    );
    // Selecting in the schematic must reach the real shared assembly selection.
    let other = description["components"]
        .as_object()
        .unwrap()
        .keys()
        .find(|other| **other != id)
        .unwrap()
        .clone();
    call(
        &schematic,
        json!([op(
            "select",
            json!({"target":{"kind":"components","ids":[other]}})
        )]),
    );
    wait(|| {
        request(&physical, "GET", "/v1/state", None).unwrap()["selection"]["ids"] == json!([other])
    });
    // Shared discussions and view links are the same persisted commands in both hosts.
    let motor = "example/motor-thermal/motor";
    let members = json!([
        "example/motor-thermal/case",
        motor,
        "example/motor-thermal/rotor"
    ]);
    let target = json!({"kind":"components","ids":members});
    let camera = json!({"focus":[0.0,0.05,0.0],"radius":0.5,"yaw":0.72,"pitch":0.5});
    call(
        &physical,
        json!([
            op("display", json!({"action":{"kind":"show_all"}})),
            op("select", json!({"target":target})),
            op("camera", camera)
        ]),
    );
    let pview = note(
        &physical,
        json!({"operation":"save_view","id":"heat-path","label":"Motor and heat path"}),
    );
    assert!(pview["views"]["heat-path"]["physical"].is_object());
    wait(|| {
        request(&schematic, "GET", "/v1/annotations", None).unwrap()["revision"]
            == pview["revision"]
    });
    call(&schematic, json!([op("select", json!({"target":target}))]));
    let joined = note(
        &schematic,
        json!({"operation":"save_view","id":"heat-path","label":"Motor and heat path"}),
    );
    assert!(joined["views"]["heat-path"]["physical"].is_object());
    assert!(joined["views"]["heat-path"]["schematic"].is_object());
    let mut invalid_view = joined["views"]["heat-path"].clone();
    invalid_view["id"] = json!("invalid-view");
    invalid_view["schematic"]["state"]["description_id"] = json!("foreign projection");
    let rejected=batch(&schematic,&json!({"commands":[op("annotations",json!({"action":{"operation":"edit","change":{"operation":"put_view","view":invalid_view}}}))]}),Duration::from_secs(5)).unwrap();
    assert_eq!(rejected["status"], "failed");
    receipts.push(rejected);
    let mut discussion = json!({"id":"heat","label":"Motor / heat path","text":"Electrical input drives the rotor. Losses heat the case; inspect these components together.","targets":target,"color":[26,135,145],"links":[{"label":"Motor","target":{"kind":"selection","target":{"kind":"components","ids":[motor]}}},{"label":"Inspect both views","target":{"kind":"view","id":"heat-path"}}]});
    let added = note(
        &schematic,
        json!({"operation":"edit","change":{"operation":"put_note","note":discussion},"expected_revision":joined["revision"]}),
    );
    wait(|| {
        request(&physical, "GET", "/v1/annotations", None).unwrap()["revision"] == added["revision"]
    });
    discussion["text"] = json!(
        "Electrical input drives the rotor. Losses heat the case. Shared annotation edited from the physical host."
    );
    let edited = note(
        &physical,
        json!({"operation":"edit","change":{"operation":"put_note","note":discussion},"expected_revision":added["revision"]}),
    );
    wait(|| {
        request(&schematic, "GET", "/v1/annotations", None).unwrap()["notes"]["heat"]["text"]
            == discussion["text"]
    });
    note(
        &physical,
        json!({"operation":"edit","change":{"operation":"undo"}}),
    );
    wait(|| {
        request(&schematic, "GET", "/v1/annotations", None).unwrap()["notes"]["heat"]["text"]
            != discussion["text"]
    });
    note(
        &schematic,
        json!({"operation":"edit","change":{"operation":"redo"}}),
    );
    wait(|| {
        request(&physical, "GET", "/v1/annotations", None).unwrap()["notes"]["heat"]["text"]
            == discussion["text"]
    });
    let mut temporary = discussion.clone();
    temporary["id"] = json!("temporary");
    note(
        &physical,
        json!({"operation":"edit","change":{"operation":"put_note","note":temporary}}),
    );
    let deleted = note(
        &schematic,
        json!({"operation":"edit","change":{"operation":"delete_note","id":"temporary"}}),
    );
    assert!(deleted["notes"].get("temporary").is_none());
    wait(|| {
        request(&physical, "GET", "/v1/annotations", None).unwrap()["notes"]
            .get("temporary")
            .is_none()
    });
    let referenced=batch(&physical,&json!({"commands":[op("annotations",json!({"action":{"operation":"edit","change":{"operation":"delete_view","id":"heat-path"}}}))]}),Duration::from_secs(5)).unwrap();
    assert_eq!(referenced["status"], "failed");
    receipts.push(referenced);
    let stale=batch(&physical,&json!({"commands":[op("annotations",json!({"action":{"operation":"edit","change":{"operation":"delete_note","id":"heat"},"expected_revision":0}}))]}),Duration::from_secs(5)).unwrap();
    assert_eq!(stale["status"], "failed");
    receipts.push(stale);
    note(
        &physical,
        json!({"operation":"follow_link","note":"heat","index":0}),
    );
    wait(|| {
        request(&schematic, "GET", "/v1/state", None).unwrap()["selected"]
            == json!({"Component":motor})
    });
    note(
        &schematic,
        json!({"operation":"emphasize","target":{"kind":"components","ids":[motor]}}),
    );
    let emphasized = call(&schematic, json!([op("state", json!({}))]));
    assert_eq!(
        emphasized["results"][0]["value"]["annotation_emphasis"]["ids"],
        json!([motor])
    );
    note(
        &schematic,
        json!({"operation":"emphasize","target":{"kind":"none"}}),
    );
    call(
        &physical,
        json!([op(
            "camera",
            json!({"focus":[0,0,0],"radius":0.8,"yaw":1.2,"pitch":0.3})
        )]),
    );
    note(
        &schematic,
        json!({"operation":"follow_link","note":"heat","index":1}),
    );
    wait(|| request(&physical, "GET", "/v1/state", None).unwrap()["selection"] == target);
    let restored = request(&physical, "GET", "/v1/state", None).unwrap();
    assert!((restored["camera"]["yaw"].as_f64().unwrap() - 0.72).abs() < 1e-5);
    wait(|| request(&schematic, "GET", "/v1/state", None).unwrap()["layout_pending"] == false);
    receipts.push(render(
        &physical,
        json!({"options":{"view":"isometric"}}),
        &evidence,
        "physical-annotated",
    ));
    receipts.push(render(&physical,json!({"options":{"view":"isometric","parts":[motor],"exploded":false,"section":{"axis":"x","offset":0.0}}}),&evidence,"physical-section"));
    assert!(
        receipts.last().unwrap()["results"][0]["value"]["metadata"]["section_caps"]
            .as_u64()
            .unwrap()
            > 0
    );
    receipts.push(render(
        &schematic,
        json!({"request":{"kind":"schematic"}}),
        &evidence,
        "schematic-annotated",
    ));
    let disk: Value = serde_json::from_slice(&std::fs::read(&annotations).unwrap()).unwrap();
    assert_eq!(disk["notes"]["heat"], discussion);
    assert!(disk["revision"].as_u64().unwrap() > edited["revision"].as_u64().unwrap());
    receipts.push(json!({"annotations_persisted":disk,"restored_physical_view":restored}));
    let annotation =
        json!({"target":{"kind":"component","id":id},"label":null,"text":"REST acceptance note"});
    receipts.push(call(
        &schematic,
        json!([
            op("annotate", json!({"annotation":annotation})),
            op("undo", json!({})),
            op("workspace", json!({})),
            op("redo", json!({})),
            op("workspace_save", json!({"path":null}))
        ]),
    ));
    assert!(
        receipts.last().unwrap()["results"][2]["value"]["annotations"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let saved: Value = serde_json::from_slice(&std::fs::read(&workspace).unwrap()).unwrap();
    assert_eq!(saved["annotations"].as_object().unwrap().len(), 1);
    let bad=batch(&schematic,&json!({"commands":[op("select",json!({"target":{"kind":"ports","ids":["missing"]}})),op("undo",json!({}))]}),Duration::from_secs(5)).unwrap();
    assert_eq!(bad["status"], "failed");
    assert_eq!(bad["completed"], 1);
    receipts.push(bad);
    let mut diagram = request(&schematic, "GET", "/v1/state", None).unwrap()["diagram"].clone();
    let node = diagram["positions"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    diagram["positions"][&node]["x"] =
        json!(diagram["positions"][&node]["x"].as_f64().unwrap() + 40.);
    diagram["pinned"] = json!([node]);
    diagram["zoom"] = json!(1.25);
    diagram["revision"] = json!(diagram["revision"].as_u64().unwrap() + 1);
    receipts.push(call(
        &schematic,
        json!([
            op("diagram", json!({"state":diagram})),
            op("state", json!({})),
            op("undo", json!({}))
        ]),
    ));
    assert_eq!(
        receipts.last().unwrap()["results"][1]["value"]["diagram"]["zoom"],
        1.25
    );
    let animation: Value = serde_json::from_slice(
        &std::fs::read(root.join("examples/systems-viewer/spatial/motor-thermal.animation.json"))
            .unwrap(),
    )
    .unwrap();
    let observables: Vec<_> = animation["readouts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["observable"].clone())
        .collect();
    receipts.push(call(&schematic,json!([
        op("simulation",json!({"action":{"command":"reset"}})),op("graphs",json!({"observables":observables,"cursor":null})),
        op("simulation",json!({"action":{"command":"step"}})),op("simulation",json!({"action":{"command":"step"}})),op("measurements",json!({})),
        op("simulation",json!({"action":{"command":"begin_recording","observables":observables,"capacity":10}})),
        op("simulation",json!({"action":{"command":"step"}})),op("simulation",json!({"action":{"command":"step"}})),op("simulation",json!({"action":{"command":"take_recording"}}))
    ])));
    let measured = &receipts.last().unwrap()["results"][4]["value"];
    assert_eq!(measured["status"]["step"], 2);
    assert_eq!(measured["frame"]["step"], 2);
    for observable in &observables {
        assert!(
            measured["frame"]["values"]
                .get(observable.as_str().unwrap())
                .is_some(),
            "requested observable missing: {observable}"
        );
    }
    wait(|| request(&physical, "GET", "/v1/measurements", None).unwrap()["step"] == 4);
    let physical_frame = request(&physical, "GET", "/v1/measurements", None).unwrap();
    assert_eq!(physical_frame["run_id"], measured["status"]["run_id"]);
    assert_eq!(
        physical_frame["generation"],
        measured["status"]["generation"]
    );
    let recording = &receipts.last().unwrap()["results"][8]["value"]["recording"];
    assert!(recording["frames"].as_array().unwrap().len() >= 2);
    assert_eq!(recording["config"]["seed"], measured["config"]["seed"]);
    receipts.push(call(
        &schematic,
        json!([
            op("simulation", json!({"action":{"command":"start"}})),
            op("simulation", json!({"action":{"command":"pause"}})),
            op("measurements", json!({}))
        ]),
    ));
    assert_eq!(
        receipts.last().unwrap()["results"][2]["value"]["status"]["phase"],
        "paused"
    );
    let bad=batch(&schematic,&json!({"commands":[op("simulation",json!({"action":{"command":"start"}})),op("simulation",json!({"action":{"command":"step"}})),op("simulation",json!({"action":{"command":"reset"}}))]}),Duration::from_secs(10)).unwrap();
    assert_eq!(bad["status"], "failed");
    assert_eq!(bad["completed"], 2);
    receipts.push(bad);
    call(
        &schematic,
        json!([op("simulation", json!({"action":{"command":"pause"}}))]),
    );
    call(
        &schematic,
        json!([op("simulation", json!({"action":{"command":"reset"}}))]),
    );
    let steps: Vec<_> = (0..30)
        .map(|_| op("simulation", json!({"action":{"command":"step"}})))
        .collect();
    call(&schematic, json!(steps));
    receipts.push(render(
        &schematic,
        json!({"request":{"kind":"graphs"}}),
        &evidence,
        "simulation-graphs",
    ));
    experiment(
        &schematic,
        json!({"operation":"open","path":root.join("examples/actuators/hx30hm/pwm-identification").display().to_string()}),
    );
    wait(|| experiment(&schematic, json!({"operation":"state"}))["busy"] == false);
    let study = experiment(&schematic, json!({"operation":"study"}));
    let trial = study["archive"]["trials"][0]["id"].clone();
    assert!(trial.is_string());
    experiment(
        &schematic,
        json!({"operation":"configure","fields":{"notes":"REST acceptance review"}}),
    );
    experiment(&schematic, json!({"operation":"evaluate","ids":[trial]}));
    wait(|| experiment(&schematic, json!({"operation":"state"}))["busy"] == false);
    let state = experiment(&schematic, json!({"operation":"state"}));
    assert!(state["error"].is_null(), "{state}");
    let study = experiment(&schematic, json!({"operation":"study"}));
    assert_eq!(study["evaluations"].as_array().unwrap().len(), 1);
    let mut view = study["view"].clone();
    view["trial_id"] = trial.clone();
    experiment(
        &schematic,
        json!({"operation":"configure","fields":{"view":view}}),
    );

    experiment(
        &schematic,
        json!({"operation":"save","path":evidence.join("experiment-review.json").display().to_string(),"html":false}),
    );
    wait(|| experiment(&schematic, json!({"operation":"state"}))["busy"] == false);
    assert!(evidence.join("experiment-review.json").is_file());
    receipts.push(render(
        &schematic,
        json!({"request":{"kind":"experiment"}}),
        &evidence,
        "experiment-graphs",
    ));
    experiment(
        &schematic,
        json!({"operation":"open","path":evidence.join("does-not-exist.json").display().to_string()}),
    );
    wait(|| experiment(&schematic, json!({"operation":"state"}))["busy"] == false);
    let error = experiment(&schematic, json!({"operation":"state"}));
    assert!(error["error"].is_string());
    receipts.push(json!({"experiment_error":error}));
    std::fs::write(&workspace, b"externally changed; preserve this file\n").unwrap();
    let conflict = batch(
        &schematic,
        &json!({"commands":[op("workspace_save",json!({"path":null}))]}),
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(conflict["status"], "failed");
    assert_eq!(
        std::fs::read(&workspace).unwrap(),
        b"externally changed; preserve this file\n"
    );
    receipts.push(conflict);
    assert_eq!(std::fs::read(&source).unwrap(), retained);
    for (path, bytes) in retained_inputs {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    call(&schematic, json!([op("simulation_cancel", json!({}))]));
    std::fs::write(
        evidence.join("receipts.json"),
        serde_json::to_vec_pretty(&receipts).unwrap(),
    )
    .unwrap();
    println!(
        "Passed actual headless REST acceptance for both viewers, runtime stepping/recording, experiments and persistence. Evidence: {}",
        evidence.display()
    );
}
