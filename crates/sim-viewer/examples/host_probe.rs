//! Native graphics compatibility probe. This is not the completed viewer.
//! SIM_VIEWER_PROBE_SCREENSHOT captures a rendered frame and exits.
use eframe::egui;
use sim_core::definitions::{FrozenDefinitions, builtins};

struct Probe {
    definitions: FrozenDefinitions,
    frames: usize,
    screenshot: Option<String>,
}

impl eframe::App for Probe {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frames += 1;
        if let Some(path) = &self.screenshot {
            ui.ctx().request_repaint();
            if self.frames == 3 {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let screenshots = ui.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| {
                        if let egui::Event::Screenshot { image, .. } = event {
                            Some(image.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            });
            for screenshot in screenshots {
                image::save_buffer(
                    path,
                    screenshot.as_raw(),
                    screenshot.width() as u32,
                    screenshot.height() as u32,
                    image::ColorType::Rgba8,
                )
                .expect("save native probe screenshot");
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Systems viewer — native host probe");
            ui.label("Registered connector definitions · graphics compatibility check");
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("connectors").striped(true).show(ui, |ui| {
                    ui.strong("Connector");
                    ui.strong("Across / through quantities");
                    ui.end_row();
                    for connector in &self.definitions.catalog().connectors {
                        if connector.lanes.is_empty() {
                            continue;
                        }
                        ui.label(&connector.label);
                        ui.label(
                            connector
                                .lanes
                                .iter()
                                .map(|lane| {
                                    let across = self
                                        .definitions
                                        .quantity(
                                            self.definitions
                                                .quantity_handle(&lane.across.quantity)
                                                .unwrap(),
                                        )
                                        .unwrap();
                                    let through = lane
                                        .through
                                        .as_ref()
                                        .map(|t| {
                                            let q = self
                                                .definitions
                                                .quantity(
                                                    self.definitions
                                                        .quantity_handle(&t.quantity)
                                                        .unwrap(),
                                                )
                                                .unwrap();
                                            format!("{} ({})", t.name, q.canonical_unit)
                                        })
                                        .unwrap_or_else(|| "—".into());
                                    format!(
                                        "{} ({}) / {through}",
                                        lane.across.name, across.canonical_unit
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                        ui.end_row();
                    }
                });
            });
        });
    }
}

fn main() -> eframe::Result {
    let definitions = builtins::registry()
        .expect("builtins")
        .freeze()
        .expect("valid definitions");
    eframe::run_native(
        "Systems viewer native probe",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1024., 900.]),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        },
        Box::new(move |_| {
            Ok(Box::new(Probe {
                definitions,
                frames: 0,
                screenshot: std::env::var("SIM_VIEWER_PROBE_SCREENSHOT").ok(),
            }))
        }),
    )
}
