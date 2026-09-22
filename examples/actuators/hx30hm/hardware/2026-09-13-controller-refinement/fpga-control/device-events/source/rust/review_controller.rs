//! Batch counterpart of the viewer's controller experiment workflow.
use sim_runtime::{
    controller_refinement::{
        control::{self, Experiment, Knot, Policy},
        recording::{self, Purpose, Recording},
    },
    experiment_comparison::{Limits, hx_archive},
    experiment_study::Study,
};
use std::{io::Write, path::Path, sync::atomic::AtomicBool};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("review-device-capture") if args.len()==4 => {
            use sim_runtime::controller_refinement::fpga_events::Capture;
            let capture:Capture=serde_json::from_slice(&std::fs::read(&args[2])?)?;
            let review=capture.review()?;
            let bytes=serde_json::to_vec_pretty(&review)?;
            let mut output=std::fs::OpenOptions::new().write(true).create_new(true).open(&args[3])?;
            output.write_all(&bytes)?;
            println!("Device record coverage complete: {}; complete frames: {}. {}",review.completed,review.completed_frames,review.completion_scope);
        },
        Some("prepare-fpga-design") if args.len()==6 => {
            use sim_runtime::controller_refinement::fpga_design::{Experiment,Profile};
            let study=Study::load(Path::new(&args[2]))?;
            let index:usize=args[3].parse()?;
            let r=study.refinement.fpga_recordings.get(index).ok_or("Missing timing recording")?;
            let profile:Profile=serde_json::from_slice(&std::fs::read(&args[4])?)?;
            let mut plan=profile.apply(&r.plan)?;
            plan.role="training".into();plan.name=format!("Design: {}",plan.name);
            let e=Experiment{timing_recording_hash:r.fingerprint(),plan};e.validate(r)?;
            let mut output=std::fs::OpenOptions::new().write(true).create_new(true).open(&args[5])?;
            output.write_all(&serde_json::to_vec_pretty(&e)?)?;
        },
        Some("design-fpga") if args.len()==5 => {
            use sim_runtime::controller_refinement::{fpga_design,calibration::Family};
            let mut study=Study::load(Path::new(&args[2]))?;
            let e:fpga_design::Experiment=serde_json::from_slice(&std::fs::read(&args[3])?)?;
            let r=study.refinement.fpga_recordings.iter().find(|r|r.fingerprint()==e.timing_recording_hash).ok_or("Missing design timing source")?;
            let run=fpga_design::simulate(&e,r,&Family{shared:study.draft.clone(),device_deltas:Default::default()},&AtomicBool::new(false),|_,_|{})?;
            println!("Simulation only: {} completed motors, {} unscored",run.axes.len(),run.failures.len());
            study.refinement.fpga_design_runs.push(run);
            study.save_new(Path::new(&args[4]))?;
            study.export_html_new(&Path::new(&args[4]).with_extension("html"))?;
        },
        Some("import-fpga") if args.len()==5 => {
            let mut study=Study::load(Path::new(&args[2]))?;
            let r:sim_runtime::controller_refinement::fpga::Recording=serde_json::from_slice(&std::fs::read(&args[3])?)?;
            r.validate_capture()?;
            if study.refinement.fpga_recordings.iter().any(|source|source.fingerprint()==r.fingerprint()) {return Err("Recording already imported".into());}
            study.refinement.fpga_recordings.push(r);
            study.save_new(Path::new(&args[4]))?;
        },
        Some("predict-fpga") if args.len()==6 => {
            use sim_runtime::controller_refinement::fpga_review::{review,Mode};
            let mut study=Study::load(Path::new(&args[2]))?;
            let index:usize=args[3].parse()?;
            let r=study.refinement.fpga_recordings.get(index).ok_or("Unknown FPGA recording index")?;
            let mode=match args[4].as_str(){"replay"=>Mode::Replay,"closed-loop"=>Mode::ClosedLoop,_=>return Err("Expected replay or closed-loop".into())};
            let result=review(r,&study.draft,mode,&AtomicBool::new(false),|_,_|{})?;
            study.validation_seen |= r.plan.role=="validation";
            study.refinement.fpga_reviews.push(result);
            study.save_new(Path::new(&args[5]))?;
            study.export_html_new(&Path::new(&args[5]).with_extension("html"))?;
        },
        Some("import-fpga-results") if args.len()==5 => {
            use sim_runtime::controller_refinement::fpga_review::{Review,Prediction};
            let mut study=Study::load(Path::new(&args[2]))?;
            let value:serde_json::Value=serde_json::from_slice(&std::fs::read(&args[3])?)?;
            let predictions:Vec<Prediction>=serde_json::from_value(value["predictions"].clone())?;
            let first=predictions.first().ok_or("Missing predictions")?;
            let result=Review{recording_hash:first.recording_blake3.clone(),models:predictions.iter().map(|p|(p.id,p.model.clone())).collect(),mode:first.mode,predictions,failures:vec![],cancelled:false};
            let r=study.refinement.fpga_recordings.iter().find(|r|r.fingerprint()==result.recording_hash).ok_or("Import the source recording first")?;
            result.validate(r)?;
            study.validation_seen |= r.plan.role=="validation";
            study.refinement.fpga_reviews.push(result);
            study.save_new(Path::new(&args[4]))?;
            study.export_html_new(&Path::new(&args[4]).with_extension("html"))?;
        },

        Some("predict-recording") if args.len()==6 => {
            let mut study=Study::load(Path::new(&args[2]))?;
            let i:usize=args[3].parse()?;
            let r=study.refinement.recordings.get(i).ok_or("Unknown recording index")?;
            let purpose=match args[4].as_str(){"replay"=>Purpose::RecordedCommandReplay,"closed-loop"=>Purpose::ClosedLoopPrediction,_=>return Err("Expected replay or closed-loop".into())};
            let limits=study.refinement.recording_assignments.iter().find(|a|a.recording_hash==r.fingerprint()).map(|a|a.limits.clone()).unwrap_or(Limits{rmse:3.*r.experiment.timing.encoder_quantum_rad,final_abs_error:5.*r.experiment.timing.encoder_quantum_rad});
            let p=recording::predict(r,&study.draft,purpose,&limits,&AtomicBool::new(false),|_,_|{})?;
            println!("Prediction {}: RMS {} rad",study.refinement.predictions.len(),p.model_error.rmse);
            study.refinement.predictions.push(p);study.save_new(Path::new(&args[5]))?;
        },
        Some("compare-electrical") if args.len()==6 => {
            use sim_runtime::controller_refinement::electrical_measurements as electrical;
            let mut study=Study::load(Path::new(&args[2]))?;
            let i:usize=args[3].parse()?;
            let p=study.refinement.predictions.get(i).ok_or("Unknown prediction index")?;
            let r=study.refinement.recordings.iter().find(|r|r.fingerprint()==p.recording_hash).ok_or("Missing recording")?;
            let m=if args[4]=="servo-voltage" {electrical::Measurements::servo_voltage(r)?}else{serde_json::from_slice(&std::fs::read(&args[4])?)?};
            m.validate_recording(r)?;
            let e=electrical::evaluate(&m,p)?;
            for c in &e.channels{println!("{} RMS {} {} passes {:?}",c.name,c.rmse,c.measured.unit,c.passes);}
            study.refinement.electrical_comparisons.push(e);study.save_new(Path::new(&args[5]))?;
            study.export_html_new(&Path::new(&args[5]).with_extension("html"))?;
        },
        Some("simulate-controller") if args.len()==5 => {
            #[derive(serde::Deserialize)]
            struct Request { experiment:Experiment, model:sim_runtime::experiment_study::ModelSettings }
            let mut study=Study::load(Path::new(&args[2]))?;
            let request:Request=serde_json::from_slice(&std::fs::read(&args[3])?)?;
            let run=control::simulate(&request.experiment,&request.model,&AtomicBool::new(false),|_,_|{})?;
            println!("Simulation failure: {:?}; tracking: {:?}; electrical: {:?}",run.failure,run.score,run.electrical.as_ref().map(|t| &t.summary));
            study.draft=request.model;study.refinement.experiment=request.experiment;
            study.refinement.controller_runs.push(run);
            study.save_new(Path::new(&args[4]))?;
            study.export_html_new(&Path::new(&args[4]).with_extension("html"))?;
        },
        Some("release-review") if args.len()==6=>{
            let coast:sim_runtime::actuator_bench::DriverRelease=serde_json::from_slice(&std::fs::read(&args[4])?)?;
            let study=sim_runtime::controller_refinement::sweep_review::load_release(Path::new(&args[2]),Path::new(&args[3]),coast,&AtomicBool::new(false),|n,total|println!("Imported release {n}/{total}"))?;
            study.save_new(Path::new(&args[5]))?;study.export_html_new(&Path::new(&args[5]).with_extension("html"))?;
        },

        Some("fit-combined") if args.len()==5=>{
            use sim_runtime::controller_refinement::{calibration,calibration_data::{Assignment,RecordingDataset,CombinedDataset,CombinedFitAttempt}};
            #[derive(serde::Deserialize)]
            struct Request { additional_studies:Vec<String>, assignments:Vec<Assignment>, fit:calibration::FitRequest }
            let mut study=Study::load(Path::new(&args[2]))?;
            let request:Request=serde_json::from_slice(&std::fs::read(&args[3])?)?;
            let mut archives=vec![study.archive.clone()];
            for path in &request.additional_studies {archives.push(Study::load(Path::new(path))?.archive);}
            for a in &request.assignments {
                if let Some(old)=study.refinement.recording_assignments.iter().find(|old|old.recording_hash==a.recording_hash) {
                    if old!=a {return Err("Cannot change original recording role, limits or rationale".into());}
                } else {study.refinement.recording_assignments.push(a.clone());}
            }
            let recordings=if request.assignments.is_empty(){None}else{Some(RecordingDataset::capture(&study.refinement.recordings,&request.assignments)?)};
            let dataset=CombinedDataset{archives,recordings};dataset.validate()?;
            let attempt=calibration::attempt(&dataset,&request.fit,&AtomicBool::new(false),|n,total|println!("Combined objective {n}/{total}"));
            if let Some(fit)=&attempt.outcome {
                println!("Fitted parameters: {}",serde_json::to_string(&fit.optimizer["values"])?);
                for score in &fit.scores {println!("ID{} {} {}: {:?} → {:?} rad RMS",score.device,score.split,score.id,score.baseline.as_ref().map(|s|s.rmse),score.candidate.as_ref().map(|s|s.rmse));}
            }
            if let Some(error)=&attempt.failure {println!("Retained failed attempt: {error}");}
            study.refinement.combined_fits.push(CombinedFitAttempt{dataset,attempt});
            study.save_new(Path::new(&args[4]))?;
        },
        Some("fit-recordings") if args.len()==5=>{
            use sim_runtime::controller_refinement::{calibration,calibration_data::{Assignment,RecordingDataset,RecordingFitAttempt}};
            #[derive(serde::Deserialize)]
            struct Request {assignments:Vec<Assignment>,fit:calibration::FitRequest}
            let mut study=Study::load(Path::new(&args[2]))?;
            let request:Request=serde_json::from_slice(&std::fs::read(&args[3])?)?;
            for a in &request.assignments {
                if let Some(old)=study.refinement.recording_assignments.iter().find(|old|old.recording_hash==a.recording_hash) {
                    if old!=a {return Err("Cannot change original recording role, limits or rationale".into());}
                } else {study.refinement.recording_assignments.push(a.clone());}
            }
            let dataset=RecordingDataset::capture(&study.refinement.recordings,&request.assignments)?;
            let attempt=calibration::attempt(&dataset,&request.fit,&AtomicBool::new(false),|n,total|println!("Recorded-command objective {n}/{total}"));
            if let Some(fit)=&attempt.outcome {
                println!("Fitted parameters: {}",serde_json::to_string(&fit.optimizer["values"])?);
                for score in &fit.scores {println!("ID{} {} {}: {:?} → {:?} rad RMS",score.device,score.split,score.id,score.baseline.as_ref().map(|s|s.rmse),score.candidate.as_ref().map(|s|s.rmse));}
            }
            if let Some(error)=&attempt.failure {println!("Retained failed attempt: {error}");}
            study.refinement.recording_fits.push(RecordingFitAttempt{dataset,attempt});
            study.save_new(Path::new(&args[4]))?;
        },
        Some("export") if args.len()==4=>{
            let study=Study::load(Path::new(&args[2]))?;
            study.export_html_new(Path::new(&args[3]))?;
            println!("Validated {} host-controller recordings and {} FPGA acquisitions; {} host predictions and {} per-motor FPGA predictions; {} fitted results; exported {}",study.refinement.recordings.len(),study.refinement.fpga_recordings.len(),study.refinement.predictions.len(),study.refinement.fpga_reviews.iter().map(|r|r.predictions.len()).sum::<usize>(),study.refinement.fits.len()+study.refinement.recording_fits.iter().filter(|f|f.attempt.outcome.is_some()).count()+study.refinement.combined_fits.iter().filter(|f|f.attempt.outcome.is_some()).count()+study.refinement.fpga_fits.iter().filter(|f|f.attempt.outcome.is_some()).count(),args[3]);
        },
        Some("evaluate-fit"|"evaluate-recording-fit"|"evaluate-combined-fit") if args.len()==5=>{
            let mut study=Study::load(Path::new(&args[2]))?;
            let index:usize=args[3].parse()?;
            let fit=if args[1]=="evaluate-combined-fit" {study.refinement.combined_fits.get(index).and_then(|f|f.attempt.outcome.as_ref()).ok_or("Unknown or failed combined fit")?}else if args[1]=="evaluate-recording-fit" {study.refinement.recording_fits.get(index).and_then(|f|f.attempt.outcome.as_ref()).ok_or("Unknown or failed recording fit")?}else{study.refinement.fits.get(index).ok_or("Unknown fit index")?}.clone();
            let devices=fit.scores.iter().map(|s|s.device).collect::<std::collections::BTreeSet<_>>();
            for r in study.refinement.recordings.iter().filter(|r|devices.contains(&r.experiment.device)) {
                let model=fit.candidate.model(r.experiment.device)?;
                let limits=study.refinement.recording_assignments.iter().find(|a|a.recording_hash==r.fingerprint()).map(|a|a.limits.clone()).unwrap_or(Limits{rmse:3.*r.experiment.timing.encoder_quantum_rad,final_abs_error:5.*r.experiment.timing.encoder_quantum_rad});
                for purpose in [Purpose::RecordedCommandReplay,Purpose::ClosedLoopPrediction] {
                    let p=recording::predict(r,&model,purpose,&limits,&AtomicBool::new(false),|_,_|{})?;
                    println!("ID{} fitted {:?}: model RMS {:.6}, peak {:.6}, passes {}",r.experiment.device,purpose,p.model_error.rmse,p.model_error.maximum_abs_error,p.model_error.passes);
                    study.refinement.predictions.push(p);
                }
            }
            study.save_new(Path::new(&args[4]))?;
        },
        Some("fit") if args.len()==5=>{
            let mut study=Study::load(Path::new(&args[2]))?;
            let request:sim_runtime::controller_refinement::calibration::FitRequest=serde_json::from_slice(&std::fs::read(&args[3])?)?;
            let attempt=sim_runtime::controller_refinement::calibration::attempt(&study.archive,&request,&AtomicBool::new(false),|n,total|println!("Objective evaluation {n}/{total}"));
            if let Some(fit)=&attempt.outcome {
                println!("Fit values: {}",serde_json::to_string(&fit.candidate.device_deltas)?);
                for score in &fit.scores {println!("{} {}: {:?} → {:?} rad",score.device,score.split,score.baseline.as_ref().map(|s|s.rmse),score.candidate.as_ref().map(|s|s.rmse));}
                study.refinement.fits.push(fit.clone());
            }
            if let Some(error)=&attempt.failure{println!("Retained failed attempt: {error}");}
            study.refinement.fit_attempts.push(attempt);study.save_new(Path::new(&args[4]))?;
        },
        Some("sweep-review") if args.len()==5=>{
            let study=sim_runtime::controller_refinement::sweep_review::load(Path::new(&args[2]),Path::new(&args[3]),&AtomicBool::new(false),|n,total|{if n%18==0{println!("Imported {n}/{total} trials");}})?;
            study.save_new(Path::new(&args[4]))?;
            println!("Saved {} trials with declared repetition splits to {}",study.archive.trials.len(),args[4]);
        },
        Some("bench-plan") if (4..=6).contains(&args.len())=>{
            let evidence=std::fs::read(&args[2])?;
            let mut e=Experiment::default();
            e.device=args.get(4).map(|s|s.parse()).transpose()?.unwrap_or(4);
            e.name="Unloaded PWM controller: small return trajectory".into();
            e.fixture="User-reported unloaded bench; no intentional external load. Attached output hardware not yet measured. Initial encoder, supply and temperature are captured at preflight.".into();
            e.duration_s=3.;e.timing.period_s=0.05;e.timing.observation_delay_ticks=0;e.timing.command_delay_ticks=0;e.timing.maximum_observation_age_s=0.1;
            e.timing.evidence="50 ms intended cadence; measured read-only transport baseline supports a conservative initial zero-action loop. Actual tick/feedback/command windows retained; sensor age unknown.".into();
            if let Policy::RustPid{parameters}=&mut e.controller {parameters.duty_limit=0.05;parameters.kp=0.8;parameters.ki=0.;parameters.kd=0.03;}
            e.trajectory=vec![Knot{time_s:0.,position_rad:0.},Knot{time_s:0.3,position_rad:0.},Knot{time_s:0.7,position_rad:0.08},Knot{time_s:1.5,position_rad:0.08},Knot{time_s:1.9,position_rad:0.},Knot{time_s:3.,position_rad:0.}];
            if args.get(5).is_some_and(|a|a=="zero") {for k in &mut e.trajectory{k.position_rad=0.;}e.name="Zero-action controller cadence measurement".into();}
            e.validate()?;
            let plan=serde_json::json!({"control":"closed_loop_pwm","experiment":e,"cadence_evidence":std::fs::canonicalize(&args[2])?,"cadence_evidence_blake3":blake3::hash(&evidence).to_hex().to_string()});
            let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&args[3])?;file.write_all(&serde_json::to_vec_pretty(&plan)?)?;file.sync_all()?;
            println!("Prepared {}. No hardware opened.",args[3]);
        },
        Some("compare") if args.len()>=5=>{
            let repository=Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let mut study=if Path::new(&args[2]).is_dir(){Study::new(hx_archive::load(Path::new(&args[2]),&repository)?)?}else{Study::load(Path::new(&args[2]))?};
            let cancel=AtomicBool::new(false);
            for path in &args[4..] {
                let r:Recording=serde_json::from_slice(&std::fs::read(path)?)?;r.validate()?;
                study.refinement.experiment=r.experiment.clone();
                for purpose in [Purpose::RecordedCommandReplay,Purpose::ClosedLoopPrediction] {
                    let p=recording::predict(&r,&study.draft,purpose,&Limits{rmse:3.*hx_archive::ENCODER_QUANTUM_RAD,final_abs_error:5.*hx_archive::ENCODER_QUANTUM_RAD},&cancel,|_,_|{})?;
                    println!("{} ID{} {:?}: model RMS {:.6} rad; peak {:.6}; pass {}",path,r.experiment.device,purpose,p.model_error.rmse,p.model_error.maximum_abs_error,p.model_error.passes);
                    study.refinement.predictions.push(p);
                }
                study.refinement.controller_runs.push(control::simulate(&r.experiment,&study.draft,&cancel,|_,_|{})?);
                study.refinement.recordings.push(r);
            }
            study.notes.push_str("\nFresh supervised PWM-controller measurements. Model prediction and controller tracking remain separate; no loaded-joint accuracy claimed.");
            study.save_new(Path::new(&args[3]))?;
            study.export_html_new(&Path::new(&args[3]).with_extension("html"))?;
        },
        _=>return Err("usage: review_controller review-device-capture CAPTURE_JSON NEW_REPORT | prepare-fpga-design REVIEW INDEX PROFILE_JSON NEW_EXPERIMENT | design-fpga REVIEW EXPERIMENT_JSON NEW_REVIEW | import-fpga REVIEW RECORDING NEW_REVIEW | import-fpga-results REVIEW PREDICTIONS NEW_REVIEW | predict-fpga REVIEW INDEX replay|closed-loop NEW_REVIEW | simulate-controller REVIEW REQUEST_JSON NEW_REVIEW | predict-recording REVIEW INDEX replay|closed-loop NEW_REVIEW | compare-electrical REVIEW PREDICTION_INDEX MEASUREMENTS_JSON_OR_servo-voltage NEW_REVIEW | bench-plan CADENCE_FILE NEW_PLAN [ID] [zero] | compare ARCHIVE_OR_STUDY NEW_REVIEW RECORDING... | sweep-review DIRECTORY SPLIT_FILE NEW_REVIEW | fit INPUT_REVIEW REQUEST_JSON NEW_REVIEW | evaluate-fit INPUT_REVIEW FIT_INDEX NEW_REVIEW | export REVIEW NEW_HTML | fit-recordings REVIEW REQUEST_JSON NEW_REVIEW | evaluate-recording-fit REVIEW FIT_INDEX NEW_REVIEW | release-review DIRECTORY SPLIT_FILE COAST_HYPOTHESIS NEW_REVIEW | fit-combined REVIEW REQUEST_JSON NEW_REVIEW | evaluate-combined-fit REVIEW FIT_INDEX NEW_REVIEW".into()),
    }
    Ok(())
}
