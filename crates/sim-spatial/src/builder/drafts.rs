//! Text drafts: opening and committing the one text field, its kit field
//! (`builder.draft`/`builder.note`), and files dropped on the window.
use super::*;
use crate::ui_kit::text::{EnterKey, FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFieldApp, TextFocus};

impl Builder {
    pub(super) fn start_input(&mut self, purpose: Purpose, initial: String) {
        if self.input.is_some(){self.status="Finish or cancel the current draft first.".into();self.action_error=Some(self.status.clone());self.panel_dirty=true;return;}
        self.input = Some(TextInput { purpose, buffer: initial });
        self.panel_dirty = true;
    }

    /// Commit the open draft (a draft that needs the selection, Position or
    /// Rename, is committed through [`Builder::commit_picking`]).
    pub(super) fn commit_input(&mut self) {
        self.commit(None);
    }

    /// Commit the open draft with the selection: Position moves the selected
    /// instances, Rename selects the new name.
    pub(super) fn commit_picking(&mut self, pick: &mut Picked) {
        self.commit(Some(pick));
    }

    fn commit(&mut self, mut pick: Option<&mut Picked>) {
        let original=self.input.clone();
        let Some(TextInput { purpose, buffer }) = self.input.take() else { return };
        self.panel_dirty = true;
        let text = buffer.trim().to_string();
        let result: Result<(), String> = match purpose {
            Purpose::CommentAuthor=>{if text.is_empty(){Err("Enter an author name".into())}else{self.discussion.author=text;Ok(())}},
            Purpose::Comment|Purpose::ThreadTitle=>Err("Use the discussion editor to submit".into()),
            Purpose::GridSpacing=>text.parse::<f32>().map_err(|_|"Enter spacing in metres".into()).and_then(|spacing_m|{let mut grid=self.grid();grid.spacing_m=spacing_m;self.set_grid(grid)}),
            Purpose::GridOrigin|Purpose::Position=>{
                let p: Result<Vec<f32>,_>=text.split_whitespace().map(str::parse).collect();
                match p {Ok(p) if p.len()==3=>{let position=[p[0],p[1],p[2]];if purpose==Purpose::GridOrigin {let mut grid=self.grid();grid.origin_m=position;self.set_grid(grid)}else{match pick.as_deref() {Some(pick)=>self.display_move(pick.names().into_iter().collect(),position,false,false,None).map(|_|()),None=>Err("Position moves the selection: commit it from the inspector".into())}}},_=>Err("Enter three coordinates in metres: x y z".into())}
            }

            Purpose::Filter => {
                self.filter = text;
                self.page = 0;
                Ok(())
            }
            Purpose::Parameter { name, parameter } => {
                if text.is_empty() {
                    self.apply("Clear parameter", vec![SystemCommand::SetParameter { at: self.level.clone(), name, parameter, binding: None }]).map(|_| ())
                } else if let Some(from) = text.strip_prefix('$') {
                    self.apply("Inherit parameter", vec![SystemCommand::SetParameter { at: self.level.clone(), name, parameter, binding: Some(sim_system::ParameterBinding::Parameter { parameter: from.to_string() }) }]).map(|_| ())
                } else {
                    match text.parse::<f64>() {
                        Ok(v) => self.apply("Set parameter", vec![SystemCommand::SetParameter { at: self.level.clone(), name, parameter, binding: Some(sim_system::ParameterBinding::value(v)) }]).map(|_| ()),
                        Err(_) => Err(format!("`{text}` is not a number (or $parameter to inherit)")),
                    }
                }
            }
            Purpose::Rename(name) => {
                let r = self.apply("Rename", vec![SystemCommand::RenameInstance { at: self.level.clone(), name, new_name: text.clone() }]).map(|_| ());
                // The renamed instance stays selected under its new name.
                if let (Ok(()), Some(pick)) = (&r, pick.as_deref_mut()) {
                    pick.sync(self);
                    let _ = pick.set([text]);
                }
                r
            }
            Purpose::ImportImage => self.import_image(PathBuf::from(text)),
            Purpose::OpenSystem => self.open_system(PathBuf::from(text)).map(|_| ()),
            Purpose::ActuatorRegistry if text.is_empty() => Err("Type the path of an actuator registry.json".into()),
            Purpose::ActuatorRegistry => self.actuators_request(Some(PathBuf::from(text)), None).map(|_| ()),
            Purpose::ActuatorConsumer if text.is_empty() => Err("Type the path of a file that embeds a robot model".into()),
            Purpose::ActuatorConsumer => self.actuators_request(None, Some(vec![PathBuf::from(text)])).map(|_| ()),
            Purpose::GaitResults if text.is_empty() => Err("Type the path of a gait-lab results folder".into()),
            Purpose::GaitResults => self.gait_reports_request(Some(PathBuf::from(text))).map(|_| ()),
            Purpose::Distance { id, first, second } => match text.parse::<f32>() {
                Ok(d) => self.apply("Calibrate reference", vec![SystemCommand::CalibrateReference { at: self.level.clone(), id, first, second, distance: d }]).map(|_| ()),
                Err(_) => Err("Enter the real distance between the two points in meters".into()),
            },
            Purpose::Sweep { name, parameter, observe } => {
                let parts: Vec<f64> = text.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                match parts[..] {
                    [from, to, count] if count >= 2. && count <= 64. => {
                        let n = count as usize;
                        let values = (0..n).map(|i| from + (to - from) * i as f64 / (n - 1) as f64).collect();
                        let study = self.default_study(observe, &name, sim_system::StudyKind::Sweep { parameter: parameter.clone(), values });
                        self.save_and_run_study(&format!("sweep_{name}_{}", parameter.replace('/', "_")), study)
                    }
                    _ => Err("Type: from to count (for example 1 4 4)".into()),
                }
            }
            Purpose::ReferenceWidth(id) => match (text.parse::<f32>(), self.reference(&id)) {
                (Ok(w), Some(mut r)) => {
                    r.width = w;
                    self.apply("Resize reference", vec![SystemCommand::SetReference { at: self.level.clone(), id, reference: r }]).map(|_| ())
                }
                _ => Err("Enter a width in meters".into()),
            },
        };
        if result.is_err(){self.input=original;}
        self.report(result);
    }
}

/// The draft's kit field: a comment or thread title is a note (Enter posts,
/// Shift+Enter is a newline); every other draft is one line (Enter commits).
pub(crate) const DRAFT: FieldId = FieldId("builder.draft");
pub(crate) const NOTE: FieldId = FieldId("builder.note");

/// Spawn the builder's two kit fields (`BuilderPlugin`). Sticky: an open
/// draft ends on Enter, Escape or its Cancel button, not on a press elsewhere.
pub(super) fn add_fields(app: &mut App) {
    app.add_message::<FieldMsg>()
        .add_text_field(DRAFT, TextField::new("Builder draft").sticky())
        .add_text_field(NOTE, TextField::new("Discussion draft").enter(EnterKey::ShiftNewline).sticky());
}

/// Which kit field types draft `purpose`.
fn field_for(purpose: &Purpose) -> FieldId {
    if matches!(purpose, Purpose::Comment | Purpose::ThreadTitle) { NOTE } else { DRAFT }
}

/// The open draft and its kit field (SimSync, Build only, after the
/// builder's handler applied this frame's actions). The draft
/// (`Builder.input`) is document state shared with `system_ui`; the field
/// is how the keyboard types it:
/// - typing (`Changed`) is mirrored into the draft's buffer;
/// - Enter (`Submit`) is the draft's submit action and Escape (`Cancel`) its
///   drop action, applied by the builder's handler; Escape also leaves
///   Connect/Annotate, as build mode's Escape key does;
/// - a draft that is open while no field has the keyboard is given it (a
///   draft just opened, or one kept across a mode switch, which took the
///   keyboard away), with its buffer; a buffer `system_ui` changed is pushed
///   into the field; a closed draft's field is blurred.
///
/// The key that opened a draft ("/" for the filter) is not typed: the kit
/// read that frame's keys before this system focused the field.
pub(super) fn sync_field(
    // The field's messages are read first, then `TextFocus` acts (a `ParamSet`: one at a time).
    mut field: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    mut builder: ResMut<Builder>,
    mut out: MessageWriter<crate::app::actions::Act<system_actions::SystemAction>>,
    mut submitted: Local<Option<(Purpose, String)>>,
) {
    let mut send = |action: BuildAction| {
        out.write(crate::app::actions::Act::ui(system_actions::SystemAction::Ui(action)));
    };
    // Escape this frame: the draft is dropped by the handler next frame, so it is not refocused meanwhile.
    let mut cancelled = false;
    // Enter last frame (the draft then): the handler has applied it since.
    // This frame's typing (keys after the Enter) is mirrored only into that
    // same draft (a refused commit restores it), not into a draft opened
    // since; otherwise the field takes the buffer again below.
    let after_submit = submitted.take();
    let same_draft = after_submit.as_ref().is_none_or(|(purpose, buffer)| builder.input.as_ref().is_some_and(|i| i.purpose == *purpose && i.buffer == *buffer));
    let msgs: Vec<FieldMsg> = field.p0().read().filter(|m| m.field == DRAFT || m.field == NOTE).cloned().collect();
    for m in &msgs {
        match &m.event {
            FieldEvent::Changed(draft) => {
                if same_draft && builder.input.as_ref().is_some_and(|i| field_for(&i.purpose) == m.field && i.buffer != draft.text) {
                    // Mutably only here: an idle frame leaves `builder` unchanged (its change detection).
                    let b = &mut *builder;
                    if let Some(input) = b.input.as_mut() {
                        input.buffer = draft.text.clone();
                    }
                    b.panel_dirty = true;
                }
            }
            FieldEvent::Submit(_) if builder.input.is_some() => {
                send(BuildAction::SubmitDraft);
                *submitted = builder.input.as_ref().map(|i| (i.purpose.clone(), i.buffer.clone()));
            }
            FieldEvent::Cancel => {
                cancelled = true;
                if builder.input.is_some() {
                    send(BuildAction::DropDraft);
                    // Escape also leaves Connect/Annotate (this runs in build mode only).
                    if builder.drag.is_none() {
                        send(BuildAction::SetMode(Mode::Select));
                    }
                }
            }
            // A blur leaves the draft open (refocused below); Tab does nothing in a draft.
            FieldEvent::Submit(_) | FieldEvent::Tab { .. } | FieldEvent::Arrow { .. } | FieldEvent::Blur => {}
        }
    }
    let mut text = field.p1();
    let mine = [DRAFT, NOTE].into_iter().find(|f| text.focused(*f));
    match builder.input.as_ref().map(|i| (field_for(&i.purpose), i.buffer.clone())) {
        Some((id, buffer)) if mine == Some(id) => {
            if text.draft(id).is_some_and(|d| d.text != buffer) {
                text.set(id, TextDraft::new(buffer, false));
            }
        }
        Some((id, buffer)) if !cancelled && (mine.is_some() || !text.typing()) => {
            text.focus_draft(id, TextDraft::new(buffer, false));
        }
        Some(_) => {}
        None => {
            if let Some(id) = mine {
                text.blur(id);
            }
        }
    }
}

/// Build mode's dropped files. Skipped on the frame Build mode is entered
/// (`State<ViewerMode>` changed): a reader gated off by `run_if` keeps its
/// cursor, so drops still buffered from the mode before (CAD's reference
/// images; messages live two updates) would be read here again.
pub(super) fn drops(mut events: MessageReader<FileDragAndDrop>, mode: Res<State<crate::app::ViewerMode>>, mut builder: ResMut<Builder>) {
    if mode.is_changed() {
        events.clear();
        return;
    }
    for e in events.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = e {
            let name = path_buf.to_string_lossy().to_lowercase();
            let result = if name.ends_with(".definition.json") {
                match library::import(path_buf) {
                    Ok(definitions) => {
                        let id = library::read(path_buf).map(|(f, _)| f.definition).unwrap_or_default();
                        let r = builder.apply("Import library definition", vec![SystemCommand::AddDefinitions { definitions }]);
                        r.map(|_| builder.status = format!("Imported {id}; it is now in the palette"))
                    }
                    Err(e) => Err(e.to_string()),
                }
            } else {
                builder.import_image(path_buf.clone())
            };
            builder.report(result);
        }
    }
}
