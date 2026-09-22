//! Offline review using the same runner as the Rust viewer. Never accesses hardware.
use sim_runtime::{
    experiment_comparison::hx_archive,
    experiment_study::{Study, evaluate},
};
use std::{path::PathBuf, sync::atomic::AtomicBool};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let directory = PathBuf::from(
        args.first()
            .ok_or("usage: review_experiments ARCHIVE NEW_REVIEW_JSON [--all]")?,
    );
    let output = PathBuf::from(args.get(1).ok_or("new output file required")?);
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut study = Study::new(hx_archive::load(&directory, &repo)?)?;
    study.draft.motor.insert("gear_friction".into(), 0.025);
    let ids = study
        .archive
        .trials
        .iter()
        .filter(|t| {
            args.iter().any(|a| a == "--all")
                || (t.device == 4 && (t.drive.abs() - 0.1).abs() < 0.001)
        })
        .map(|t| t.id.clone())
        .collect::<Vec<_>>();
    let run = evaluate(
        &study.archive,
        &ids,
        &study.baseline,
        &study.draft,
        None,
        false,
        &AtomicBool::new(false),
        |n, total| {
            if n % 25 == 0 || n == total {
                eprintln!("{n}/{total} trials");
            }
        },
    )?;
    let s = run.summary(&ids);
    eprintln!(
        "{} passes; {} failures; {} unscored; {} RMSE regressions",
        s.passes, s.failures, s.unscored, s.regressed
    );
    study.validation_seen = true;
    study.view.trial_id = ids.first().cloned();
    study.view.evaluation = Some(0);
    study.notes="Demonstration candidate, not accepted calibration. Physical baseline and candidate replay reconstructed PWM inputs; incomplete fixture/controller conditions remain exploratory.".into();
    study.evaluations.push(run);
    study.save_new(&output)?;
    study.export_html_new(&output.with_extension("html"))?;
    if s.unscored > 0 {
        return Err("Some physical trials could not be scored; inspect retained results".into());
    }
    Ok(())
}
