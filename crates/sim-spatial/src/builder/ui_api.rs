//! `system_ui` for the builder: the live controls (every rendered button's
//! `BuildAction`, collected after the panel is rebuilt) and the handler of
//! `system_actions::UiAction`, which activates a control's action through
//! `dispatch` or makes the gesture a click would, without pixel input.
use super::*;
use super::system_actions::UiAction;
use serde::Serialize;
use std::hash::{Hash, Hasher};

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Enabled(pub bool);
#[derive(Clone, Serialize)]
struct Control {
    id: String,
    label: String,
    enabled: bool,
    /// Descriptive intent; Activate dispatches this exact cloned action.
    action: BuildAction,
}
pub(super) struct Controls {
    revision: u64,
    digest: u64,
    items: BTreeMap<String, Control>,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            revision: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64,
            digest: 0,
            items: BTreeMap::new(),
        }
    }
}
fn hash(value: &impl Serialize) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    serde_json::to_vec(value).unwrap_or_default().hash(&mut h);
    h.finish()
}
impl Builder {
    pub(super) fn ui_state(&self) -> serde_json::Value {
        serde_json::json!({"tab":self.tab,"mode":self.mode,"controls_revision":self.ui_api.revision,"controls_ready":!self.panel_dirty&&!self.ui_api.items.is_empty(),"draft":self.input.as_ref().map(|i|serde_json::json!({"purpose":i.purpose,"text":i.buffer})),"last_error":self.action_error})
    }
    /// The control `system_ui` activate names, checked as a click would be:
    /// the listed controls must be current (`ui_revision`), and it must exist
    /// and be enabled.
    fn activated(&self, id: &str, ui_revision: u64) -> Result<Control, String> {
        if self.panel_dirty || ui_revision != self.ui_api.revision {
            return Err("UI changed; request controls again before activating".into());
        }
        let c = self.ui_api.items.get(id).ok_or("unknown control; request controls")?.clone();
        if !c.enabled {
            return Err(format!("control is disabled: {}", c.label));
        }
        Ok(c)
    }
    /// Whether `request` activates the "‹ lesson" control, whose button writes
    /// the mode switch to Lessons (`actions::buttons`) rather than a builder
    /// action: its activation goes to the same switch (`system_actions`).
    /// Errors are `ui_request`'s for the same request.
    pub(crate) fn activates_lessons(&self, request: &UiAction, expected_revision: Option<u64>) -> Result<bool, String> {
        let UiAction::Activate { id, ui_revision } = request else { return Ok(false) };
        if expected_revision.is_some_and(|r| r != self.document.revision) {
            return Err("stale system revision; read system_state".into());
        }
        Ok(matches!(self.activated(id, *ui_revision)?.action, BuildAction::Lessons))
    }
    pub(crate) fn ui_request(
        &mut self,
        request: UiAction,
        expected_revision: Option<u64>,
        scene: &mut SpatialScene,
        orbit: &mut Orbit,
    ) -> Result<serde_json::Value, String> {
        if expected_revision.is_some_and(|r| r != self.document.revision) {
            return Err("stale system revision; read system_state".into());
        }
        self.action_error = None;
        match request {
            UiAction::Controls => {
                return Ok(
                    serde_json::json!({"ui_revision":self.ui_api.revision,"ready":!self.panel_dirty&&!self.ui_api.items.is_empty(),"controls":self.ui_api.items.values().collect::<Vec<_>>(),"state":self.ui_state()}),
                );
            }
            UiAction::Activate { id, ui_revision } => {
                // The "‹ lesson" control never gets here: `system_actions` sends it
                // to the mode switch, as its button does (`activates_lessons`).
                let c = self.activated(&id, ui_revision)?;
                dispatch(self, scene, orbit, c.action);
            }
            UiAction::Tab { tab } => dispatch(self, scene, orbit, BuildAction::Tab(tab)),
            UiAction::Mode { mode } => {
                if self.input.is_some() {
                    return Err("finish or cancel the current draft first".into());
                }
                dispatch(self, scene, orbit, BuildAction::SetMode(mode));
            }
            UiAction::OpenThread { id } => {
                if !self.document.discussions.threads.contains_key(&id) {
                    return Err("unknown discussion".into());
                }
                dispatch(
                    self,
                    scene,
                    orbit,
                    BuildAction::Discussion(discussion::Action::Open(id)),
                );
            }
            UiAction::ClickPart {
                component,
                add,
                point_m,
            } => {
                sim_system::display::bind(&self.document, &component).map_err(|e| e.to_string())?;
                if self.mode == Mode::Annotate {
                    self.begin_annotation_target(scene, &component, point_m.unwrap_or([0.; 3]))?;
                } else {
                    if self.instance_for_component(&component).is_none() {
                        return Err("part is outside the current level; use system_level".into());
                    }
                    click_part(self, &component, add);
                }
            }
            UiAction::Annotate { target, pin_m } => {
                self.begin_annotation_target(scene, &target, pin_m)?
            }
            UiAction::Input {
                text,
                expected_text,
                submit,
            } => {
                let input = self
                    .input
                    .as_mut()
                    .ok_or("no input is open; activate the field or Reply first")?;
                if input.buffer != expected_text {
                    return Err(
                        "draft changed; read system_state.ui.draft before replacing text".into(),
                    );
                }
                if text.len() > 65536 {
                    return Err("input exceeds 65536 bytes".into());
                }
                input.buffer = text;
                self.panel_dirty = true;
                if submit {
                    if matches!(
                        self.input.as_ref().unwrap().purpose,
                        Purpose::Comment | Purpose::ThreadTitle
                    ) {
                        discussion::submit(self, scene, orbit);
                    } else {
                        self.commit_input();
                    }
                }
            }
            UiAction::CancelInput { expected_text } => {
                let input = self.input.as_ref().ok_or("no input is open")?;
                if input.buffer != expected_text {
                    return Err(
                        "draft changed; read system_state.ui.draft before cancelling".into(),
                    );
                }
                discussion::act(self, scene, orbit, discussion::Action::CancelDraft);
            }
            UiAction::Scroll { offset_y } => {
                if !offset_y.is_finite() {
                    return Err("offset_y must be a finite number of pixels".into());
                }
                self.sidebar_scroll = Some(offset_y.max(0.0));
                self.panel_dirty = true;
            }
        }
        if let Some(e) = &self.action_error {
            return Err(e.clone());
        }
        Ok(self.state_json())
    }
    fn begin_annotation_target(
        &mut self,
        scene: &SpatialScene,
        path: &str,
        pin: [f32; 3],
    ) -> Result<(), String> {
        if self.input.is_some() {
            return Err("finish or cancel the current draft first".into());
        }
        if !pin.iter().all(|v| v.is_finite()) {
            return Err("annotation pin requires finite local metres".into());
        }
        sim_system::display::bind(&self.document, path).map_err(|e| e.to_string())?;
        if let Some(index) = scene.spatial.parts.iter().position(|p| p.component == path) {
            let world =
                animation::part_transform(scene, index).transform_point(Vec3::from_array(pin));
            discussion::begin_surface(self, scene, index, world);
        } else {
            // Groups have no pickable surface; the same draft workflow uses their local frame.
            self.discussion.draft_targets = vec![path.into()];
            self.discussion.draft_pin = Some(pin);
            self.discussion.selected = None;
            self.discussion.editing = None;
            self.discussion.selected_only = false;
            self.discussion.reset_scroll = true;
            self.tab = Tab::Discussions;
            self.mode = Mode::Select;
            self.start_input(Purpose::Comment, String::new());
        }
        Ok(())
    }
}

/// Collect actual controls after rebuilding the UI, including viewport markers.
/// The generation binds index-based palette/snap actions to the inspected UI.
pub(super) fn collect(
    mut b: ResMut<Builder>,
    controls: Query<(Entity, &BuildAction, Option<&Enabled>), With<Button>>,
    children: Query<&Children>,
    texts: Query<&Text>,
) {
    if b.panel_dirty {
        return;
    }
    fn labels(
        entity: Entity,
        children: &Query<&Children>,
        texts: &Query<&Text>,
        out: &mut Vec<String>,
        depth: usize,
    ) {
        if depth > 10 {
            return;
        }
        if let Ok(text) = texts.get(entity) {
            out.push(text.0.clone());
        }
        if let Ok(kids) = children.get(entity) {
            for child in kids.iter() {
                labels(child, children, texts, out, depth + 1);
            }
        }
    }
    let mut items: BTreeMap<String, Control> = BTreeMap::new();
    for (entity, action, enabled) in &controls {
        let mut text = vec![];
        labels(entity, &children, &texts, &mut text, 0);
        let id = format!("control-{:016x}", hash(action));
        let label = text.join(" · ");
        let candidate = Control {
            id: id.clone(),
            label,
            enabled: enabled.is_none_or(|e| e.0),
            action: action.clone(),
        };
        // Duplicate buttons with the same semantic action are one logical control.
        items
            .entry(id)
            .and_modify(|c| {
                c.enabled |= candidate.enabled;
                // Shortest label, ties broken by text so the result does not depend on
                // ECS iteration order (which changed between Bevy 0.16 and 0.19).
                if (candidate.label.len(), &candidate.label) < (c.label.len(), &c.label) {
                    c.label = candidate.label.clone();
                }
            })
            .or_insert(candidate);
    }
    let digest = hash(
        &serde_json::json!({"revision":b.document.revision,"level":b.level,"selected":b.selected,"tab":b.tab,"mode":b.mode,"controls":items}),
    );
    if digest != b.ui_api.digest {
        b.ui_api.revision += 1;
        b.ui_api.digest = digest;
        b.ui_api.items = items;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rest_ui_actions_share_click_handlers_and_protect_drafts_and_stale_controls() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("builder-ui-api-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("review.system.json");
        std::fs::copy(
            root.join("examples/systems-builder/worm-drive/winch.system.json"),
            &path,
        )
        .unwrap();
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let mut b = Builder::open(path, root.join("library/systems"), registry.clone()).unwrap();
        let compiled = system_builder::compile(
            &b.document,
            &registry,
            system_builder::config_for(&b.document),
        )
        .unwrap();
        let spatial = compiled
            .spatial
            .clone()
            .unwrap_or_else(|| compiled.flat.spatial(&compiled.description.id, "Review"));
        let mut scene = SpatialScene::for_builder(compiled.description, spatial).unwrap();
        let mut orbit = Orbit {
            focus: Vec3::ZERO,
            radius: 0.5,
            yaw: 0.5,
            pitch: 0.5,
            home: false,
            ..Default::default()
        };
        b.ui_request(
            UiAction::Mode {
                mode: Mode::Connect,
            },
            None,
            &mut scene,
            &mut orbit,
        )
        .unwrap();
        b.ui_request(
            UiAction::ClickPart {
                component: "motor".into(),
                add: false,
                point_m: None,
            },
            None,
            &mut scene,
            &mut orbit,
        )
        .unwrap();
        assert_eq!(b.port_menu.as_deref(), Some("motor"));
        assert!(b.selected.contains("motor"));
        let before = b.document.clone();
        b.ui_request(
            UiAction::Annotate {
                target: "motor".into(),
                pin_m: [0.001, 0.002, 0.003],
            },
            None,
            &mut scene,
            &mut orbit,
        )
        .unwrap();
        assert_eq!(b.tab, Tab::Discussions);
        assert!(
            b.ui_request(
                UiAction::Input {
                    text: "stale".into(),
                    expected_text: "some other draft".into(),
                    submit: false
                },
                None,
                &mut scene,
                &mut orbit
            )
            .is_err()
        );
        assert_eq!(b.input.as_ref().unwrap().buffer, "");
        assert_eq!(b.document, before);
        b.ui_request(
            UiAction::Input {
                text: "REST surface note".into(),
                expected_text: "".into(),
                submit: true,
            },
            None,
            &mut scene,
            &mut orbit,
        )
        .unwrap();
        let id = b.discussion.selected.clone().unwrap();
        assert_eq!(
            b.document.discussions.threads[&id].comments[0].body,
            "REST surface note"
        );
        assert!(
            (Vec3::from_array(b.document.discussions.threads[&id].pin_m.unwrap())
                - Vec3::new(0.001, 0.002, 0.003))
            .length()
                < 1e-6
        );
        b.tab = Tab::Library;
        b.ui_request(
            UiAction::OpenThread { id: id.clone() },
            None,
            &mut scene,
            &mut orbit,
        )
        .unwrap();
        assert_eq!(b.tab, Tab::Discussions);
        // Actual ECS controls are discoverable; disabled and stale controls fail.
        let mut app = App::new();
        app.insert_resource(b);
        app.add_systems(Update, collect);
        app.world_mut().spawn((
            Button,
            BuildAction::Discussion(discussion::Action::Reply),
            Enabled(false),
            children![(Text::new("Write a reply…"),)],
        ));
        app.world_mut().resource_mut::<Builder>().panel_dirty = false;
        app.update();
        let mut b = app.world_mut().remove_resource::<Builder>().unwrap();
        let c = b.ui_api.items.values().next().unwrap().clone();
        let ui_revision = b.ui_api.revision;
        // The collected id fits the builder's registered control pattern.
        let patterns = <super::super::system_actions::SystemAction as crate::app::actions::Action>::controls();
        assert!(patterns.iter().any(|p| crate::app::actions::control_matches(p, &c.id)), "{}", c.id);
        assert!(c.label.contains("reply"));
        assert!(!c.enabled);
        assert!(
            b.ui_request(
                UiAction::Activate {
                    id: c.id.clone(),
                    ui_revision
                },
                None,
                &mut scene,
                &mut orbit
            )
            .is_err()
        );
        b.ui_api.items.get_mut(&c.id).unwrap().enabled = true;
        assert!(
            b.ui_request(
                UiAction::Activate {
                    id: c.id.clone(),
                    ui_revision: ui_revision - 1
                },
                None,
                &mut scene,
                &mut orbit
            )
            .is_err()
        );
        b.ui_request(
            UiAction::Activate {
                id: c.id,
                ui_revision,
            },
            None,
            &mut scene,
            &mut orbit,
        )
        .unwrap();
        assert_eq!(b.input.as_ref().unwrap().purpose, Purpose::Comment);
        b.ui_request(
            UiAction::Input {
                text: "Draft to keep".into(),
                expected_text: "".into(),
                submit: false,
            },
            None,
            &mut scene,
            &mut orbit,
        )
        .unwrap();
        assert!(
            b.ui_request(
                UiAction::CancelInput {
                    expected_text: "wrong".into()
                },
                None,
                &mut scene,
                &mut orbit
            )
            .is_err()
        );
        assert_eq!(b.input.as_ref().unwrap().buffer, "Draft to keep");
        b.ui_request(
            UiAction::CancelInput {
                expected_text: "Draft to keep".into(),
            },
            None,
            &mut scene,
            &mut orbit,
        )
        .unwrap();
        assert!(b.input.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
