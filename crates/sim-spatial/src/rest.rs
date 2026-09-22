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
    use sim_api::capability as c;
    let mut capabilities = capabilities();
    if builder {
        capabilities.extend([
            c("system", json!({"label":"Place resistor","commands":[{"command":"add_instance","at":"","name":"r1","instance":{"kind":{"kind":"element","component_type":"electrical.resistor"},"parameters":{"resistance":{"value":100}}}}]}),
                "Apply sim-system commands atomically (same validation and shared undo history as both viewers and the CLI)"),
            c("system_state", json!({}), "System file, revision, build level, selection, findings and compile status"),
            c("system_level", json!({"path":"regulator"}), "Drill into a subsystem instance path (\"\" is the top level)"),
            c("system_select", json!({"names":["q1"]}), "Select instances at the current level"),
            c("system_undo", json!({}), "Undo the last edit in the shared history"),
            c("system_redo", json!({}), "Redo in the shared history"),
            c("system_run", json!({"action":"start"}), "Start or pause the background run on the shared runtime"),
            c("system_import_image", json!({"path":"/abs/board.png"}), "Import a PNG/JPEG as a reference image at the current level"),
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
                "SpatialCommand: select, clear_selection, set_exploded, set_connections, hide_selected, show_all",
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
    System {
        #[serde(default)]
        label: Option<String>,
        commands: Vec<sim_system::Command>,
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
}
fn system_execute(builder: &mut builder::Builder, scene: &SpatialScene, command: &sim_api::Command) -> sim_api::Result {
    match sim_api::decode::<SystemRequest>(command)? {
        SystemRequest::System { label, commands } => {
            let label = label.unwrap_or_else(|| format!("{} command(s) via REST", commands.len()));
            builder.apply(&label, commands).map(|a| json!(a))
        }
        SystemRequest::SystemState => Ok(builder.state_json()),
        SystemRequest::SystemLevel { path } => builder.set_level(&path).map(|_| builder.state_json()),
        SystemRequest::SystemSelect { names } => {
            builder.select(names);
            Ok(builder.state_json())
        }
        SystemRequest::SystemUndo => {
            builder.undo();
            Ok(builder.state_json())
        }
        SystemRequest::SystemRedo => {
            builder.redo();
            Ok(builder.state_json())
        }
        SystemRequest::SystemRun { action } => {
            match action.as_str() {
                "start" => builder.run_start(scene),
                "pause" => builder.run_pause(),
                other => return Err(format!("unknown run action `{other}` (start or pause)")),
            }
            Ok(builder.state_json())
        }
        SystemRequest::SystemImportImage { path } => builder.import_image(path).map(|_| builder.state_json()),
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
) {
    server.poll(|command, continuation, cancelled| {
        if command.command.starts_with("system") {
            return match builder.as_deref_mut() {
                Some(b) => system_execute(b, scene, command).into(),
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
    rest: Option<ResMut<Rest>>,
    mut scene: ResMut<SpatialScene>,
    mut camera: Single<&mut Orbit>,
    mut builder: Option<ResMut<builder::Builder>>,
) {
    if let Some(mut rest) = rest {
        let Rest(server, task) = &mut *rest;
        tick(server, &mut scene, &mut camera, task, builder.as_deref_mut());
    }
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
        tick(&mut server, &mut scene, &mut camera, &mut image_task, None);
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
