//! REST adapter uses the same source selection and display commands as input.
use super::*;
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Annotations {
        action: sim_inspect::annotations::Request,
    },
    Render {
        #[serde(default)]
        options: sim_render::physical::Options,
    },
    State,
    Description,
    Spatial,
    Animation,
    Measurements,
    Select {
        target: SelectionTarget,
    },
    Display {
        action: SpatialCommand,
    },
    Camera {
        focus: [f32; 3],
        radius: f32,
        yaw: f32,
        pitch: f32,
    },
    Fit,
    Panels {
        parts: bool,
        compact: bool,
    },
}
#[derive(Resource)]
pub struct Rest(pub sim_api::Server, pub Option<sim_api::ImageTask>);
pub fn server(port: u16) -> std::io::Result<sim_api::Server> {
    server_with(port, false)
}
/// Build mode adds the shared system-editing commands.
pub fn server_with(port: u16, builder: bool) -> std::io::Result<sim_api::Server> {
    server_for(port, builder, false)
}
/// Lesson mode adds the `lesson_*` commands to build mode's.
pub fn server_for(port: u16, builder: bool, lessons: bool) -> std::io::Result<sim_api::Server> {
    use sim_api::capability as c;
    let mut capabilities = capabilities();
    if lessons {
        capabilities.extend(crate::lesson::rest::capabilities());
    }
    if builder {
        capabilities.extend([
            c("system_context",json!({"discussion":"thread-ID"}),"Read-only engineering context for an annotation or targets [instance/path]. Shared with Codex: inherited values, authored provenance, typed ports, complete nets, connected neighbors, registry explanations, model findings and display-only geometry. Resolved off the UI thread; no selection, model or undo changes. Empty args inspect the model. Poll the returned job URL."),
            c("system_agent",json!({"action":{"operation":"status"}}),"Codex annotation service: status/configure(auto_answer)/ask(discussion,question?,request_id?)/cancel(run)/retry(run)/mark_read(discussion)/activity. Shared UI actions; model/effort from SIM_CODEX_MODEL/SIM_CODEX_EFFORT (default gpt-6-astra/high, reported in status), read-only answer mode. GET /v1/agent and /v1/events/agent provide live state."),
            c("system_ui",json!({"action":{"operation":"controls"}}),"Discover live controls and activate them via the exact UI handlers. Also tab/mode/click_part/annotate/open_thread/input/cancel_input. Activate requires control id and ui_revision; input uses expected_text to protect drafts. Physical placement remains display-only."),
            c("system_discussions",json!({"action":{"operation":"list"}}),"CAD-style threads: list/get/create/reply/edit_comment/delete_comment/resolve/delete/link/pin/title/show/highlight/inspect_target/back/import_legacy. Persistent part/group links; shared undo; optional expected_revision. Drafts are never replaced by REST."),
            c("system_grid",json!({}),"Read/set display-only grid (metres, Y up, enclosing definition frame). Never changes physics or CAD geometry. Optional expected_revision."),
            c("system_move",json!({"names":["motor"],"position_m":[0.04,0,0.02],"snap":true,"preview":true}),"Display-only move: first named instance is the anchor, others keep their offsets. Shared mouse/REST snapping, overlap report (allowed=false for invalid preview; commit rejects), atomic undo, expected_revision. Does NOT change physics/CAD."),
            c("system", json!({"label":"Place resistor","commands":[{"command":"add_instance","at":"","name":"r1","instance":{"kind":{"kind":"element","component_type":"electrical.resistor"},"parameters":{"resistance":{"value":100}}}}]}),
                "Apply sim-system commands atomically (same validation and shared undo history as both viewers and the CLI)"),
            c("system_state", json!({}), "System file, revision, build level, selection, findings and compile status"),
            c("system_level", json!({"path":"regulator"}), "Drill into a subsystem instance path (\"\" is the top level)"),
            c("system_select", json!({"names":["q1"]}), "Select instances at the current level"),
            c("system_undo", json!({}), "Undo the last edit in the shared history"),
            c("system_redo", json!({}), "Redo in the shared history"),
            c("system_run", json!({"action":"start"}), "Start or pause the background run on the shared runtime"),
            c("system_import_image", json!({"path":"/abs/board.png"}), "Import a PNG/JPEG as a reference image at the current level"),
            c("system_suggest", json!({"instance":"motor"}), "What can snap onto each port of an instance at the current level (typed, curated first, conflicts explained)"),
            c("system_snap", json!({"instance":"motor","port":"shaft","kind":{"kind":"element","component_type":"rotational.worm_gear"}}), "Place a fitting part next to an instance and connect it to that port (one undoable edit)"),
            c("system_component", json!({"component_type":"rotational.worm_gear"}), "Library entry: ports, parameters, notes, equations, trade-offs and derived values"),
            c("system_study", json!({"name":"gearboxes"}), "Run a saved comparison or sweep in the background (pass `study` to save it first); overlays results in the graph dock"),
            c("system_study_result", json!({}), "Latest study result: variants, metrics, derived values, trade-off table; `running` while in progress"),
            c("system_publish", json!({"definition":"dc_motor_12v"}), "Publish a definition to the library as a new version (refreshes files that bundle it)"),
            c("system_library_updates", json!({}), "Imported definitions whose library file changed"),
            c("system_sync", json!({}), "Update every stale import from the library (one undoable edit)"),
            c("system_where_used", json!({"definition":"dc_motor_12v"}), "System files under examples/ and next to this file that place a definition"),
            c("system_expose", json!({"instance":"winding","parameter":"resistance"}), "Expose an inner parameter as a parameter of the current level's definition"),
            c("system_save_run", json!({"note":"after the k edit"}), "Keep the current run (document, seed, settings, recorded history) in <system>.runs/"),
            c("system_compare_runs", json!({"ids":["…","…"]}), "Overlay saved runs in the graph dock with a table of final values"),
            c("system_parts", json!({}), "Authored part files (library/parts/*.part): load results, errors with file:line; reloads changed files"),
            c("system_plot", json!({"pin":["drum.shaft.speed"],"visible":true}), "Pin observables (IDs or readable keys) to the graph dock, or clear with []"),
        ]);
    }
    sim_api::Server::bind(port, "physical-assembly", capabilities)
}
fn capabilities() -> Vec<Value> {
    use sim_api::capability as c;
    vec![
            c(
                "annotations",
                json!({"action":{"operation":"document"}}),
                "Shared multi-part notes, links, saved views and hover emphasis; same sidecar as schematic",
            ),
            c(
                "render",
                json!({"options":{"view":"isometric","size":{"width":1280,"height":900},"section":null}}),
                "Off-screen PNG of captured geometry; x/y/z sections use meters and do not change the viewport",
            ),
            c(
                "screenshot",
                json!({"path":"/tmp/view.png"}),
                "Save the window exactly as drawn (UI, overlays, lesson pages) to a PNG after the next frame",
            ),
            c(
                "state",
                json!({}),
                "Display, selection, camera and live status",
            ),
            c(
                "description",
                json!({}),
                "Shared typed components, ports, nets, observables and validation",
            ),
            c(
                "spatial",
                json!({}),
                "Display parts, dimensions, provenance and source identities",
            ),
            c("animation", json!({}), "Source-bound observation bindings"),
            c(
                "measurements",
                json!({}),
                "Last measured frame; this viewer never advances physics",
            ),
            c(
                "select",
                json!({"target":{"kind":"components","ids":["source-id"]}}),
                "Exact shared selection: none, components, ports or nets",
            ),
            c(
                "display",
                json!({"action":{"kind":"set_exploded","enabled":true}}),
                "SpatialCommand: select, clear_selection, set_exploded, set_connections, set_overlay {layer: power|forces|current|heat|trails, enabled}, hide_selected, show_all",
            ),
            c(
                "camera",
                json!({"focus":[0,0,0],"radius":0.5,"yaw":0.7,"pitch":0.4}),
                "Absolute orbit; SI meters and radians; shared by pan, orbit and zoom",
            ),
            c("fit", json!({}), "Fit all display geometry"),
            c(
                "panels",
                json!({"parts":true,"compact":false}),
                "Parts panel and compact inspector presentation",
            ),
        ]
}
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum SystemRequest {
    SystemAgent {action:builder::agent::Request},
    SystemUi {action:builder::ui_api::Request,#[serde(default)]expected_revision:Option<u64>},
    SystemDiscussions { action: builder::discussion::Request, #[serde(default)] expected_revision: Option<u64> },
    SystemGrid { #[serde(default)] grid: Option<sim_system::display::Grid>, #[serde(default)] expected_revision: Option<u64> },
    SystemMove { names: Vec<String>, position_m: [f32;3], #[serde(default)] snap: bool, #[serde(default)] preview: bool, #[serde(default)] expected_revision: Option<u64> },

    System {
        #[serde(default)]
        label: Option<String>,
        commands: Vec<sim_system::Command>,
        #[serde(default)] expected_revision: Option<u64>,
    },
    SystemState,
    SystemLevel {
        path: String,
    },
    SystemSelect {
        names: Vec<String>,
    },
    SystemUndo,
    SystemRedo,
    SystemRun {
        action: String,
    },
    SystemImportImage {
        path: std::path::PathBuf,
    },
    SystemSuggest {
        instance: String,
    },
    SystemSnap {
        instance: String,
        port: String,
        kind: sim_system::InstanceKind,
    },
    SystemComponent {
        component_type: String,
    },
    SystemStudy {
        name: String,
        #[serde(default)]
        study: Option<sim_system::Study>,
    },
    SystemStudyResult,
    SystemParts,
    SystemPublish {
        definition: String,
    },
    SystemLibraryUpdates,
    SystemSaveRun {
        #[serde(default)]
        note: String,
    },
    SystemCompareRuns {
        ids: Vec<String>,
    },
    SystemSync,
    SystemWhereUsed {
        definition: String,
    },
    SystemExpose {
        instance: String,
        parameter: String,
    },
    SystemPlot {
        #[serde(default)]
        pin: Option<Vec<String>>,
        #[serde(default)]
        visible: Option<bool>,
    },
}
fn system_execute(builder: &mut builder::Builder, scene: &mut SpatialScene, camera: &mut Orbit, command: &sim_api::Command) -> sim_api::Result {
    match sim_api::decode::<SystemRequest>(command)? {
        SystemRequest::SystemAgent{action}=>builder.agent_request(action),
        SystemRequest::SystemUi{action,expected_revision}=>builder.ui_request(action,expected_revision,scene,camera),
        SystemRequest::SystemDiscussions{action,expected_revision}=>builder.discussion_request(action,expected_revision,scene,camera),
        SystemRequest::System { label, commands, expected_revision } => {
            if expected_revision.is_some_and(|r|r!=builder.document.revision){return Err("stale system revision; reload system_state".into());}
            let label = label.unwrap_or_else(|| format!("{} command(s) via REST", commands.len()));
            builder.apply(&label, commands).map(|a| json!(a))
        }
        SystemRequest::SystemGrid { grid, expected_revision } => {
            if expected_revision.is_some_and(|r|r!=builder.document.revision){return Err("stale display grid; reload system_state".into());}
            if let Some(grid)=grid {builder.set_grid(grid)?;}
            Ok(json!({"grid":builder.grid(),"semantics":sim_system::display::SEMANTICS,"frame":"enclosing_definition","unit":"m","revision":builder.document.revision}))
        }
        SystemRequest::SystemMove {names,position_m,snap,preview,expected_revision}=>builder.display_move(names,position_m,snap,preview,expected_revision),
        SystemRequest::SystemState => Ok(builder.state_json()),
        SystemRequest::SystemLevel { path } => builder.set_level(&path).map(|_| builder.state_json()),
        SystemRequest::SystemSelect { names } => {
            builder.select(names);
            Ok(builder.state_json())
        }
        SystemRequest::SystemUndo => builder.undo().map(|a| json!(a)),
        SystemRequest::SystemRedo => builder.redo().map(|a| json!(a)),
        SystemRequest::SystemRun { action } => {
            match action.as_str() {
                "start" => builder.run_start(scene),
                "pause" => builder.run_pause(),
                other => return Err(format!("unknown run action `{other}` (start or pause)")),
            }
            Ok(builder.state_json())
        }
        SystemRequest::SystemImportImage { path } => builder.import_image(path).map(|_| builder.state_json()),
        SystemRequest::SystemSuggest { instance } => builder.suggestions(&instance).map(|s| json!(s)),
        SystemRequest::SystemSnap { instance, port, kind } => {
            let candidate = builder
                .suggestions(&instance)?
                .into_iter()
                .find(|p| p.port == port)
                .ok_or_else(|| format!("{instance} has no port `{port}`"))?
                .candidates
                .into_iter()
                .find(|c| c.kind == kind)
                .ok_or_else(|| format!("{} does not fit {instance}.{port}", sim_system::commands::kind_label(&kind)))?;
            let name = builder.snap(&instance, &port, &candidate)?;
            Ok(json!({"name": name, "state": builder.state_json()}))
        }
        SystemRequest::SystemComponent { component_type } => builder.component_json(&component_type),
        SystemRequest::SystemStudy { name, study } => {
            match study {
                Some(study) => builder.save_and_run_study(&name, study)?,
                None => builder.run_study(&name)?,
            }
            Ok(json!({"running": name}))
        }
        SystemRequest::SystemStudyResult => Ok(builder.study_json()),
        SystemRequest::SystemPublish { definition } => builder.publish(&definition).map(|p| json!(p)),
        SystemRequest::SystemSaveRun { note } => builder.save_run(&note).map(|p| json!({"path": p})),
        SystemRequest::SystemCompareRuns { ids } => builder.compare_runs(&ids).map(|_| builder.study_json()),
        SystemRequest::SystemLibraryUpdates => Ok(json!(builder.library_updates())),
        SystemRequest::SystemSync => builder.sync_library().map(|a| json!(a)),
        SystemRequest::SystemWhereUsed { definition } => Ok(json!(builder.where_used(&definition))),
        SystemRequest::SystemExpose { instance, parameter } => builder.expose(&instance, &parameter).map(|a| json!(a)),
        SystemRequest::SystemParts => {
            builder.reload_parts();
            Ok(builder.parts_json())
        }
        SystemRequest::SystemPlot { pin, visible } => builder.set_plots(scene, pin, visible),
    }
}
fn state(scene: &SpatialScene, camera: &Orbit) -> Value {
    json!({"description_id":scene.description.id,"selection":scene.selection,"display":scene.state,
        "camera":{"focus":camera.focus.to_array(),"radius":camera.radius,"yaw":camera.yaw,"pitch":camera.pitch,"fit_pending":camera.home},
        "annotations_revision":scene.note_document().revision,"annotation_emphasis":scene.note_hover,"annotation_error":scene.note_error,"parts_visible":scene.parts_visible,"compact":scene.compact,"live_status":scene.live_status(),"live_error":scene.live.error})
}
fn execute(
    scene: &mut SpatialScene,
    camera: &mut Orbit,
    command: &sim_api::Command,
) -> sim_api::Result {
    match sim_api::decode::<Command>(command)? {
        Command::Annotations { .. } | Command::Render { .. } => {
            return Err("render requires the asynchronous image dispatcher".into());
        }
        Command::State => return Ok(state(scene, camera)),
        Command::Description => return Ok(json!(scene.description)),
        Command::Spatial => return Ok(json!(scene.spatial)),
        Command::Animation => return Ok(json!(scene.animation)),
        Command::Measurements => return Ok(json!(scene.frame())),
        Command::Select { target } => scene.set_selection(target).map_err(|e| e.to_string())?,
        Command::Display { action } => {
            let fit = matches!(action, SpatialCommand::SetExploded { .. });
            scene.apply(action).map_err(|e| e.to_string())?;
            camera.home |= fit;
        }
        Command::Camera {
            focus,
            radius,
            yaw,
            pitch,
        } => {
            if !focus
                .iter()
                .chain([radius, yaw, pitch].iter())
                .all(|x| x.is_finite())
                || radius <= 0.
                || pitch.abs() > 1.5
            {
                return Err(
                    "finite camera required; radius > 0 and pitch within ±1.5 radians".into(),
                );
            }
            camera.focus = Vec3::from_array(focus);
            camera.radius = radius;
            camera.yaw = yaw;
            camera.pitch = pitch;
            camera.home = false;
        }
        Command::Fit => camera.home = true,
        Command::Panels { parts, compact } => {
            scene.parts_visible = parts;
            scene.compact = compact;
            camera.home = true;
        }
    }
    Ok(state(scene, camera))
}
fn tick(
    server: &mut sim_api::Server,
    scene: &mut SpatialScene,
    camera: &mut Orbit,
    task: &mut Option<sim_api::ImageTask>,
    mut builder: Option<&mut builder::Builder>,
    mut learn: Option<&mut crate::lesson::Learn>,
    shots: &mut Vec<std::path::PathBuf>,
) {
    if let Some(b)=builder.as_deref_mut(){b.agent_endpoint(format!("http://{}",server.address));}
    server.poll(|command, continuation, cancelled| {
        if command.command == "screenshot" {
            let path = command.args.get("path").and_then(|p| p.as_str()).map(std::path::PathBuf::from);
            return sim_api::Outcome::Done(match path.filter(|p| p.extension().is_some_and(|e| e == "png")) {
                Some(p) => {
                    shots.push(p.clone());
                    Ok(json!({"path": p, "note": "saved once the next frame renders"}))
                }
                None => Err("screenshot needs {\"path\": \"…/file.png\"}".into()),
            });
        }
        if command.command == "lesson_frames" {
            let Some(l) = learn.as_deref_mut() else { return sim_api::Outcome::Done(Err("start the viewer with --lessons DIR to use lessons".into())) };
            if cancelled {
                l.frames = None;
                return sim_api::Outcome::Done(Err("cancelled".into()));
            }
            if continuation.is_null() {
                let started = serde_json::from_value::<crate::lesson::frames::FramesRequest>(command.args.clone()).map_err(|e| e.to_string()).and_then(|req| l.start_frames(req));
                return match started {
                    Ok(()) => {
                        *continuation = json!({"capturing": true});
                        sim_api::Outcome::Pending
                    }
                    Err(e) => sim_api::Outcome::Done(Err(e)),
                };
            }
            return crate::lesson::frames::finish(l).unwrap_or(sim_api::Outcome::Pending);
        }
        if command.command.starts_with("lesson_") {
            return match learn.as_deref_mut() {
                Some(l) => crate::lesson::rest::execute(l, scene, command).into(),
                None => sim_api::Outcome::Done(Err("start the viewer with --lessons DIR to use lessons".into())),
            };
        }
        if command.command == "system_context" {
            return match builder.as_deref_mut() {
                Some(b) => match serde_json::from_value::<sim_model_context::Request>(command.args.clone()) {
                    Ok(request) => b.context_request(request, continuation, cancelled),
                    Err(e) => sim_api::Outcome::Done(Err(e.to_string())),
                },
                None => sim_api::Outcome::Done(Err("start the viewer with --system FILE to inspect systems".into())),
            };
        }
        if command.command.starts_with("system") {
            return match builder.as_deref_mut() {
                Some(b) => system_execute(b, scene, camera, command).into(),
                None => sim_api::Outcome::Done(Err("start the viewer with --system FILE to edit systems".into())),
            };
        }
        if command.command == "annotations" {
            return match sim_api::decode::<Command>(command) {
                Ok(Command::Annotations { action }) => {
                    notes::api(scene, camera, action, continuation)
                }
                Err(e) => sim_api::Outcome::Done(Err(e)),
                _ => unreachable!(),
            };
        }
        if command.command == "render" {
            if task.is_none() {
                let options = match sim_api::decode::<Command>(command) {
                    Ok(Command::Render { options }) => options,
                    Ok(_) => unreachable!(),
                    Err(e) => return sim_api::Outcome::Done(Err(e)),
                };
                let snapshot = match capture(scene, camera, &options) {
                    Ok(s) => s,
                    Err(e) => return sim_api::Outcome::Done(Err(e)),
                };
                *task = Some(sim_api::ImageTask::spawn(move || {
                    sim_render::physical::render(&snapshot, &options).map(|r| sim_api::Artifact {
                        png: r.png,
                        metadata: r.metadata,
                    })
                }));
                *continuation = json!(true);
            }
            let result = task.as_mut().unwrap().poll(cancelled);
            if !matches!(result, sim_api::Outcome::Pending) {
                *task = None;
            }
            return result;
        }
        execute(scene, camera, command).into()
    });
    if server.snapshot_due() {
        if let Some(b)=builder.as_deref_mut(){server.publish("agent",b.agent_json());}
        if let Some(l)=learn.as_deref(){server.publish("lesson",crate::lesson::rest::state(l));}

        server.publish("state", state(scene, camera));
        server.publish("annotations", json!(scene.note_document()));
        server.publish_changed("description", &scene.description.id, || {
            json!(scene.description)
        });
        server.publish_changed("spatial", &scene.description.id, || json!(scene.spatial));
        server.publish("measurements", json!(scene.frame()));
    }
}
pub(super) fn poll(
    mut commands: Commands,
    mut redraw: EventWriter<bevy::window::RequestRedraw>,
    rest: Option<ResMut<Rest>>,
    mut scene: ResMut<SpatialScene>,
    mut camera: Single<&mut Orbit>,
    mut builder: Option<ResMut<builder::Builder>>,
    mut learn: Option<ResMut<crate::lesson::Learn>>,
) {
    if let Some(mut rest) = rest {
        let Rest(server, task) = &mut *rest;
        let mut shots = Vec::new();
        tick(server, &mut scene, &mut camera, task, builder.as_deref_mut(), learn.as_deref_mut(), &mut shots);
        // Keep frames coming while a job runs; an idle background window
        // otherwise steps only on its slow low-power timer.
        if server.busy() || !shots.is_empty() {
            redraw.write(bevy::window::RequestRedraw);
        }
        // The window as drawn (UI, overlays and all), saved on the render thread.
        for path in shots {
            use bevy::render::view::screenshot::{Screenshot, save_to_disk};
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        }
    }
}
/// macOS naps background apps: timers and wake-ups are delayed by seconds.
/// A viewer with a REST server must answer promptly, so it declares a
/// user-initiated, latency-critical activity for as long as it runs (the
/// system may still sleep when idle).
fn keep_responsive() {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        let info = NSProcessInfo::processInfo();
        let reason = NSString::from_str("Answering REST commands");
        let token = info.beginActivityWithOptions_reason(NSActivityOptions::UserInitiatedAllowingIdleSystemSleep | NSActivityOptions::LatencyCritical, &reason);
        // Held for the life of the process.
        std::mem::forget(token);
    }
}

/// Wake the event loop when a REST command arrives, so a background window
/// answers promptly without drawing continuously.
pub(super) fn wake_on_request(rest: Option<Res<Rest>>, proxy: Option<Res<bevy::winit::EventLoopProxyWrapper<bevy::winit::WakeUp>>>) {
    let (Some(rest), Some(proxy)) = (rest, proxy) else { return };
    keep_responsive();
    let proxy = std::sync::Mutex::new((**proxy).clone());
    rest.0.set_waker(move || {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(bevy::winit::WakeUp);
        }
    });
}

/// Runs the identical command adapter without creating a window or GPU context.
pub fn headless(
    mut scene: SpatialScene,
    mut link: Option<SelectionLink>,
    mut server: sim_api::Server,
) -> ! {
    let (focus, radius) = scene.bounds();
    let mut camera = Orbit {
        focus,
        radius: radius * 2.9,
        yaw: 0.7,
        pitch: 0.4,
        home: false,
        ..Default::default()
    };
    let mut image_task = None;
    loop {
        if let Some(link) = &mut link {
            if let Ok(target) = link.0.exchange(scene.selection.clone()) {
                if target != scene.selection {
                    let _ = scene.set_selection(target);
                }
            }
        }
        scene.poll_live();
        notes::sync(&mut scene, &mut camera);
        if camera.home {
            let (focus, radius) = scene.bounds();
            camera.focus = focus;
            camera.radius = radius * 2.9;
            camera.home = false;
        }
        tick(&mut server, &mut scene, &mut camera, &mut image_task, None, None, &mut Vec::new());
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> SpatialScene {
        SpatialScene::new(
            serde_json::from_str(include_str!(
                "../../../examples/systems-viewer/spatial/motor-thermal.description.json"
            ))
            .unwrap(),
            serde_json::from_str(include_str!(
                "../../../examples/systems-viewer/spatial/motor-thermal.spatial.json"
            ))
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn rest_selection_display_and_camera_preserve_source() {
        let mut scene = scene();
        let before = json!(scene.description);
        let mut camera = Orbit {
            focus: Vec3::ZERO,
            radius: 1.,
            yaw: 0.,
            pitch: 0.,
            home: false,
            ..Default::default()
        };
        let id = scene.spatial.parts[0].component.clone();
        for (name, args) in [
            ("select", json!({"target":{"kind":"components","ids":[id]}})),
            ("display", json!({"action":{"kind":"hide_selected"}})),
        ] {
            execute(
                &mut scene,
                &mut camera,
                &sim_api::Command {
                    command: name.into(),
                    args,
                },
            )
            .unwrap();
        }
        assert!(scene.state.hidden.contains(&id));
        assert!(
            execute(
                &mut scene,
                &mut camera,
                &sim_api::Command {
                    command: "camera".into(),
                    args: json!({"focus":[0,0,0],"radius":-1,"yaw":0,"pitch":0})
                }
            )
            .is_err()
        );
        assert_eq!(camera.radius, 1.);
        assert_eq!(json!(scene.description), before);
    }
}

fn capture(
    scene: &SpatialScene,
    camera: &Orbit,
    options: &sim_render::physical::Options,
) -> Result<sim_render::physical::Snapshot, String> {
    options.size.validate()?;
    if options.parts.iter().any(|id| {
        !scene
            .spatial
            .parts
            .iter()
            .any(|p| &p.id == id || &p.component == id)
    }) {
        return Err("unknown component or part in render filter".into());
    }
    let hover = if scene.note_pointer_hover != SelectionTarget::None {
        &scene.note_pointer_hover
    } else {
        &scene.note_hover
    };
    let emphasized = hover
        .resolve(&scene.description)
        .map_err(|e| e.to_string())?
        .components;
    let mut parts = Vec::new();
    for (i, p) in scene.spatial.parts.iter().enumerate() {
        if !options.include_hidden && scene.state.hidden.contains(&p.component) {
            continue;
        }
        if !options.parts.is_empty()
            && !options.parts.contains(&p.id)
            && !options.parts.contains(&p.component)
        {
            continue;
        }
        let mut transform = animation::part_transform(scene, i);
        if let Some(exploded) = options.exploded {
            if exploded != scene.state.exploded {
                transform.translation +=
                    Vec3::from_array(p.exploded_offset) * if exploded { 1. } else { -1. };
            }
        }
        parts.push(sim_render::physical::Part {
            id: p.id.clone(),
            component: p.component.clone(),
            label: scene.description.components[&p.component].label.clone(),
            shape: p.shape.clone(),
            position: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
            color: animation::part_color(scene, i).unwrap_or(p.color_srgb),
            selected: emphasized.contains(&p.component)
                || scene.details.components.contains(&p.component),
        });
    }
    let mut connections = Vec::new();
    if options.connections.unwrap_or(scene.state.connections) {
        let positions: BTreeMap<_, _> = parts
            .iter()
            .map(|p| (p.component.clone(), Vec3::from_array(p.position)))
            .collect();
        for net in scene.description.nets.values() {
            let components: std::collections::BTreeSet<_> = net
                .ports
                .iter()
                .filter_map(|id| scene.description.ports.get(id).map(|p| &p.component))
                .collect();
            let points: Vec<_> = components
                .iter()
                .filter_map(|id| positions.get(*id))
                .copied()
                .collect();
            if points.len() > 1 {
                let hub = points.iter().copied().sum::<Vec3>() / points.len() as f32;
                connections.extend(points.into_iter().map(|p| (p.to_array(), hub.to_array())));
            }
        }
    }
    Ok(sim_render::physical::Snapshot {
        parts,
        regions: scene
            .note_document()
            .notes
            .values()
            .filter_map(|n| {
                Some(sim_render::Region {
                    label: n.label.clone(),
                    color: n.color,
                    components: n.targets.resolve(&scene.description).ok()?.components,
                })
            })
            .collect(),
        connections,
        yaw: camera.yaw,
        pitch: camera.pitch,
        metadata: json!({"annotations":scene.note_document(),"source_description_id":scene.description.id,"selection":scene.selection,"frame":scene.frame().map(|f|json!({"run_id":f.run_id,"generation":f.generation,"sequence":f.sequence,"step":f.step,"time":f.time})),"live_status":scene.live_status(),"geometry":"illustrative display primitives; not source CAD solids"}),
    })
}
