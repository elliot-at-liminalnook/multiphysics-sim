//! The scene: background compiles of the document, the respawned parts, nets
//! and reference images, part and reference clicks, and clearing the
//! chrome while a lesson is shown.
use super::*;

fn compile_job(document: SystemDocument, registry: BehaviorRegistry, base: PathBuf) -> crate::jobs::Job<CompileResult> {
    crate::jobs::Job::spawn(crate::jobs::Pool::Compute, document.revision, "the compile", move |_| Ok(compile_now(document, registry, &base)))
}

/// One compile of `document` (stored in `base`) for the scene (call off the
/// UI thread): generated robots built, FMU blocks imported and bound, so a
/// changed or unusable artifact is the compile error.
pub(super) fn compile_now(document: SystemDocument, registry: BehaviorRegistry, base: &std::path::Path) -> CompileResult {
    let config = system_builder::config_for(&document);
    let result = system_builder::compile_at(&document, &registry, config.clone(), base).map(|compiled| {
        let runtime_error = system_builder::source(&compiled, &registry, &document).build(&config).err().map(|e| system_builder::locate(&compiled.flat, e));
        CompileOutput {
            spatial: compiled.spatial.clone().unwrap_or_else(|| compiled.flat.spatial(&compiled.description.id, &document.title)),
            description: compiled.description,
            animation: compiled.animation,
            findings: compiled.flat.findings.clone(),
            subsystems: compiled.flat.subsystems.clone(),
            runtime_error,
        }
    });
    // A system that is still being wired fails to compile on its first
    // loose port; say which ports are loose instead of the solver's id.
    let findings = if result.is_err() { sim_system::Resolver::new(&document, &registry).findings() } else { Vec::new() };
    let loose: Vec<&str> = findings.iter().filter(|f| f.code == "unconnected_port").map(|f| f.message.trim_end_matches(" is not connected")).collect();
    let result = result.map_err(|e| {
        if loose.is_empty() || !e.contains("is not connected") {
            e
        } else {
            format!("{} unconnected port{}: {}{}", loose.len(), if loose.len() == 1 { "" } else { "s" }, loose.iter().take(4).copied().collect::<Vec<_>>().join(", "), if loose.len() > 4 { ", …" } else { "" })
        }
    });
    CompileResult { revision: document.revision, result, findings }
}

/// Build mode's scene of `builder`'s document, compiled with the shared
/// runtime: the launch, a lessons launch and a switch to build or lessons
/// mode all start from it.
pub fn compiled_scene(builder: &Builder) -> Result<SpatialScene, String> {
    let compiled = system_builder::compile_at(&builder.document, builder.registry(), system_builder::config_for(&builder.document), &builder.system_dir()).map_err(|e| e.to_string())?;
    let spatial = compiled.spatial.clone().unwrap_or_else(|| compiled.flat.spatial(&compiled.description.id, &builder.document.title));
    let mut scene = SpatialScene::for_builder(compiled.description.clone(), spatial).map_err(|e| e.to_string())?;
    if let Some(animation) = compiled.animation.clone() {
        scene.set_animation(animation).map_err(|e| e.to_string())?;
    }
    Ok(scene)
}

/// While a lesson is shown, the builder's chrome and pins are removed; they
/// are rebuilt when the builder is shown again.
pub(super) fn clear_for_learn(mut commands: Commands, mut builder: ResMut<Builder>, chrome: Query<Entity, Or<(With<BuilderPanel>, With<markers::Marker>, With<markers::Leader>)>>) {
    for e in &chrome {
        commands.entity(e).try_despawn();
    }
    if !builder.panel_dirty {
        builder.panel_dirty = true;
    }
    if builder.drag.is_some() {
        builder.drag = None;
    }
}

/// Recompile after edits and respawn the parts, nets and reference images.
#[allow(clippy::too_many_arguments)]
pub(super) fn rebuild_scene(
    mut commands: Commands,
    mut builder: ResMut<Builder>,
    mut scene: ResMut<SpatialScene>,
    content: Query<Entity, With<SceneContent>>,
    ui_roots: Query<Entity, With<UiRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut orbit: Single<&mut Orbit>,
    mut models: Option<ResMut<crate::models::ModelLibrary>>,
    mode: Option<Res<State<ViewerMode>>>,
    selection: Res<Selection>,
    registry: Res<DocumentRegistry>,
) {
    let learning = mode.is_some_and(|m| *m.get() == ViewerMode::Lessons);
    // Do not replace picked entities while a pointer owns them. A saved drop
    // explicitly allows the replacement while its preview remains visible.
    if builder.drag.as_ref().is_some_and(|d| !d.awaiting_scene()) { return; }
    // Compile off the UI thread; apply the newest finished result.
    if builder.scene_dirty && builder.job.is_none() {
        builder.scene_dirty = false;
        builder.job = Some(compile_job(builder.document.clone(), builder.registry.clone(), builder.system_dir()));
    }
    let Some(polled) = builder.job.as_ref().and_then(crate::jobs::Job::poll) else { return };
    builder.job = None;
    let finished = match polled {
        Ok(r) => r,
        Err(e) => {
            // The compile panicked: say so instead of leaving the scene silently stale.
            builder.compile_error = Some(e.clone());
            builder.status = e;
            builder.panel_dirty = true;
            return;
        }
    };
    if finished.revision != builder.document.revision {
        // An edit arrived while compiling; compile again before redrawing.
        builder.scene_dirty = true;
        return;
    }
    let compiled = match finished.result {
        Ok(c) => c,
        Err(e) => {
            builder.compile_error = Some(e.clone());
            builder.findings = finished.findings;
            builder.status = format!("Does not compile yet: {e}");
            builder.panel_dirty = true;
            return;
        }
    };
    let first = scene.description.components.is_empty() && scene.spatial.parts.is_empty();
    builder.findings = compiled.findings;
    builder.subsystems = compiled.subsystems;
    builder.compile_error = compiled.runtime_error;
    // A running system takes the edit live: parameters keep its state,
    // structural edits restart it at t = 0 (the session decides).
    let new_id = compiled.description.id.clone();
    // The run keeps its fidelity: a realtime run takes the edit's realtime profile.
    if builder.run.as_ref().is_some_and(|r| r.description_id != new_id) {
        let document = builder.document.clone();
        builder.hot_swap(&document, new_id);
        scene.live.snapshot = None;
    }
    builder.last_description = Some(compiled.description.clone());
    builder.schematic.set_source(&compiled.description, finished.revision);
    scene.replace(compiled.description, compiled.spatial, compiled.animation);
    let level = builder.level.clone();
    scene.ghost = scene
        .spatial
        .parts
        .iter()
        .filter(|p| !(level.is_empty() || p.component == level || p.component.starts_with(&format!("{level}/"))))
        .map(|p| p.component.clone())
        .collect();
    // The new scene shows the selection (`replace` cleared its highlight);
    // in Lessons the lesson page draws its own highlight (`picked::track`).
    if !learning {
        picked::project(&builder, &mut scene, &picked::names(&selection, &registry));
    }
    for e in &content {
        commands.entity(e).despawn();
    }
    for e in &ui_roots {
        commands.entity(e).despawn();
    }
    spawn_parts(&mut commands, &scene, &mut meshes, &mut materials, models.as_deref_mut());
    linked::spawn_nets(&mut commands, &scene, &mut meshes, &mut materials);
    // Reference images of the current level, as textured planes.
    let definition = builder.definition_id();
    let references: Vec<(String, sim_system::ReferenceImage)> = definition
        .and_then(|id| builder.document.definitions.get(&id))
        .map(|d| d.references.iter().filter(|(_, r)| r.view == ReferenceView::Spatial && r.visible).map(|(k, r)| (k.clone(), r.clone())).collect())
        .unwrap_or_default();
    let frame = builder.subsystems.get(&builder.level).copied().unwrap_or(sim_system::flatten::WorldPlacement::IDENTITY);
    for (id, reference) in references {
        let Some(asset) = builder.document.assets.get(&reference.asset).cloned() else { continue };
        let texture = match builder.textures.get(&reference.asset) {
            Some(h) => h.clone(),
            None => {
                let path = sim_system::assets::resolve(&builder.store.path, &asset);
                let loaded = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| {
                    let extension = if asset.media_type == "image/png" { "png" } else { "jpg" };
                    Image::from_buffer(&bytes, ImageType::Extension(extension), CompressedImageFormats::NONE, true, ImageSampler::default(), RenderAssetUsages::default()).map_err(|e| e.to_string())
                });
                match loaded {
                    Ok(image) => {
                        let handle = images.add(image);
                        builder.textures.insert(reference.asset.clone(), handle.clone());
                        handle
                    }
                    Err(e) => {
                        builder.status = format!("Could not load reference {id}: {e}");
                        continue;
                    }
                }
            }
        };
        let height = reference.height(&asset);
        let n = Vec3::from_array(reference.normal).normalize_or(Vec3::Y);
        let x = Vec3::from_array(reference.x_axis).reject_from(n).normalize_or(Vec3::X);
        let y = n.cross(x);
        let local = Transform::from_translation(Vec3::from_array(reference.origin)).with_rotation(Quat::from_mat3(&Mat3::from_cols(x, y, n)));
        let parent = Transform::from_translation(Vec3::from_array(frame.position)).with_rotation(Quat::from_array(frame.rotation_xyzw));
        let mut entity = commands.spawn((
            Mesh3d(meshes.add(Rectangle::new(reference.width, height))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(1., 1., 1., reference.opacity),
                base_color_texture: Some(texture),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            })),
            parent * local,
            ReferenceQuad(id.clone()),
            SceneContent,
        ));
        if !reference.locked {
            entity.insert(Pickable::default()).observe(pick_reference);
        }
    }
    // Build mode draws its own chrome (spawn_ui returns at once for a builder scene).
    builder.panel_dirty = true;
    if first || !builder.fitted {
        builder.fitted = true;
        // A lesson frames its scene with the scene's own camera.
        if !learning {
            orbit.home = true;
        }
    }
}

/// A click on a reference image, as the builder's `ReferencePoint` action.
fn pick_reference(click: On<Pointer<Click>>, quads: Query<&ReferenceQuad>, mut out: MessageWriter<crate::app::actions::Act<system_actions::SystemAction>>) {
    let Ok(quad) = quads.get(click.entity) else { return };
    let Some(position) = click.hit.position else { return };
    out.write(crate::app::actions::Act::ui(system_actions::SystemAction::Ui(BuildAction::ReferencePoint { id: quad.0.clone(), world: position.to_array() })));
}

/// Part clicks select instances at the current level (shift toggles), through
/// the shared selection; a refusal is the status line.
pub(crate) fn click_part(builder: &mut Builder, pick: &mut Picked, component: &str, shift: bool) {
    if builder.drag.is_some(){return;}
    let Some(name) = builder.instance_for_component(component) else {
        builder.status = "That part is outside this level; press Up (U) to leave the subsystem.".into();
        builder.panel_dirty = true;
        return;
    };
    let result = if shift {
        pick.toggle(name)
    } else if builder.connect_from.is_some() || builder.mode == Mode::Connect {
        builder.port_menu = Some(name.clone());
        pick.set([name])
    } else {
        builder.port_menu = None;
        pick.set([name])
    };
    if let Err(e) = result {
        builder.report(Err::<(), _>(e));
        return;
    }
    if let Some(n) = pick.only() {
        let _ = builder.suggestions(&n);
    }
    builder.alternatives = None;
    builder.panel_dirty = true;
}
