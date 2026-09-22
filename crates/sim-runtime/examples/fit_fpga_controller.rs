//! FPGA recordings use the existing bounded calibration engine and immutable roles.
use sim_runtime::controller_refinement::{
    calibration::*, calibration_data::CalibrationData, fpga::Dataset,
};
use std::{fs, io::Write, sync::atomic::AtomicBool};
#[derive(serde::Serialize, serde::Deserialize)]
struct Request {
    dataset: Dataset,
    fit: FitRequest,
}
fn save(path: &str, value: &impl serde::Serialize) -> Result<(), Box<dyn std::error::Error>> {
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    f.write_all(&serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().collect::<Vec<_>>();
    match a.get(1).map(String::as_str) {
        Some("prepare-device") if a.len() >= 8 => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Settings {
                coordinates: Vec<Coordinate>,
                maximum_evaluations: usize,
                validation_influenced: bool,
                #[serde(default)]
                measured_voltage: bool,
            }
            let model: Family = serde_json::from_slice(&fs::read(&a[2])?)?;
            let device: u8 = a[3].parse()?;
            let settings: Settings = serde_json::from_slice(&fs::read(&a[4])?)?;
            model.model(device)?;
            if !(4..=12).contains(&device)
                || settings.coordinates.is_empty()
                || settings.coordinates.len() > 16
                || settings.coordinates.iter().any(|c| c.device != Some(device))
                || !(3..=1000).contains(&settings.maximum_evaluations)
            {
                return Err("Require 1–16 coordinates for the selected motor and 3–1000 evaluations".into());
            }
            let dataset = Dataset {
                measured_voltage: settings.measured_voltage,
                recordings: a[6..].iter().map(|p| Ok(serde_json::from_slice(&fs::read(p)?)?))
                    .collect::<Result<_, Box<dyn std::error::Error>>>()?,
            };
            let cases = dataset.cases()?;
            let training_ids: Vec<_> = cases.iter().filter(|c| c.device == device && c.split == "train").map(|c| c.id.clone()).collect();
            let validation_ids: Vec<_> = cases.iter().filter(|c| c.device == device && c.split != "train").map(|c| c.id.clone()).collect();
            if training_ids.is_empty() || validation_ids.is_empty() {
                return Err("Selected motor needs original training and separate validation recordings".into());
            }
            save(&a[5], &Request { dataset, fit: FitRequest {
                model, training_ids, validation_ids, coordinates: settings.coordinates,
                maximum_evaluations: settings.maximum_evaluations,
                validation_influenced: settings.validation_influenced,
            }})?;
        }
        Some("prepare") if a.len() >= 6 => {
            let model: Family = serde_json::from_slice(&fs::read(&a[2])?)?;
            let dataset = Dataset {
                measured_voltage: false,
                recordings: a[4..]
                    .iter()
                    .map(|p| Ok(serde_json::from_slice(&fs::read(p)?)?))
                    .collect::<Result<_, Box<dyn std::error::Error>>>()?,
            };
            let cases = dataset.cases()?;
            if cases.iter().any(|c| c.device != 4) {
                return Err(
                    "Focused example config fits ID4; use explicit coordinates for other devices"
                        .into(),
                );
            }
            let request = Request {
                dataset,
                fit: FitRequest {
                    model,
                    training_ids: cases
                        .iter()
                        .filter(|c| c.split == "train")
                        .map(|c| c.id.clone())
                        .collect(),
                    validation_ids: cases
                        .iter()
                        .filter(|c| c.split != "train")
                        .map(|c| c.id.clone())
                        .collect(),
                    coordinates: vec![
                        Coordinate {
                            path: "motor.no_load_current".into(),
                            device: Some(4),
                            lower: -0.099,
                            upper: 0.1,
                        },
                        Coordinate {
                            path: "motor.rotor_inertia".into(),
                            device: Some(4),
                            lower: -2e-8,
                            upper: 2e-6,
                        },
                    ],
                    maximum_evaluations: 60,
                    validation_influenced: false,
                },
            };
            save(&a[3], &request)?;
        }
        Some("fit") if a.len() == 4 => {
            let r: Request = serde_json::from_slice(&fs::read(&a[2])?)?;
            let fit = attempt(
                &r.dataset,
                &r.fit,
                &AtomicBool::new(false),
                |done, total| eprintln!("fit {done}/{total}"),
            );
            save(
                &a[3],
                &serde_json::json!({"dataset":r.dataset,"attempt":fit}),
            )?;
        }
        _ => return Err(
            "fit_fpga_controller prepare-device MODEL ID SETTINGS NEW_REQUEST RECORDING... | prepare MODEL NEW_REQUEST RECORDING... | fit REQUEST NEW_ATTEMPT"
                .into(),
        ),
    }
    Ok(())
}
