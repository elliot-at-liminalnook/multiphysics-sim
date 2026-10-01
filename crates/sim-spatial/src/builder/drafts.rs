//! Text drafts: opening and committing the one text field, typing into it,
//! and files dropped on the window.
use super::*;

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
            Purpose::CalibrationArchive if text.is_empty() => Err("Type the path of an identification archive folder".into()),
            Purpose::CalibrationArchive => self.calibration_request(Some(PathBuf::from(text))).map(|_| ()),
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

/// Typing into the open draft (input editing); Enter and Escape are the
/// draft's submit and drop actions, applied by the builder's handler.
/// Keys from the frame a draft opens are dropped: the key that opened it
/// ("/" for the filter) was pressed before the draft existed.
pub(super) fn text_input(mut events: MessageReader<KeyboardInput>, mut builder: ResMut<Builder>, keys: Res<ButtonInput<KeyCode>>, mode: Option<Res<State<ViewerMode>>>, mut was_open: Local<bool>, mut out: MessageWriter<crate::app::actions::Act<system_actions::SystemAction>>) {
    let mut send = |action: BuildAction| {
        out.write(crate::app::actions::Act::ui(system_actions::SystemAction::Ui(action)));
    };
    let opened_now = builder.input.is_some() && !*was_open;
    *was_open = builder.input.is_some();
    if builder.input.is_none() || opened_now {
        events.clear();
        return;
    }
    for e in events.read() {
        if e.state != ButtonState::Pressed {
            continue;
        }
        match &e.logical_key {
            Key::Enter => {
                if builder.input.as_ref().is_some_and(|i|matches!(i.purpose,Purpose::Comment|Purpose::ThreadTitle)) && (keys.pressed(KeyCode::ShiftLeft)||keys.pressed(KeyCode::ShiftRight)) {
                    builder.input.as_mut().unwrap().buffer.push('\n');builder.panel_dirty=true;continue;
                }
                send(BuildAction::SubmitDraft);
                return;
            }
            Key::Escape => {
                send(BuildAction::DropDraft);
                // Escape also leaves Connect/Annotate, as build mode's Escape key does.
                if builder.drag.is_none() && mode.is_some_and(|m| *m.get() == ViewerMode::Build) {
                    send(BuildAction::SetMode(Mode::Select));
                }
                return;
            }
            Key::Backspace => {
                if let Some(i) = builder.input.as_mut() {
                    i.buffer.pop();
                }
            }
            Key::Space => {
                if let Some(i) = builder.input.as_mut() {
                    i.buffer.push(' ');
                }
            }
            Key::Character(c) => {
                if let Some(i) = builder.input.as_mut() {
                    i.buffer.push_str(c.as_str());
                }
            }
            _ => {}
        }
        builder.panel_dirty = true;
    }
}

pub(super) fn drops(mut events: MessageReader<FileDragAndDrop>, mut builder: ResMut<Builder>) {
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
