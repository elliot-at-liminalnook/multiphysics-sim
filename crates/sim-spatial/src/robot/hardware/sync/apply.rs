//! The Real motor sync section's actions ([`apply`]).
use super::{SCALES, mapping};
use crate::robot::hardware::{Hardware, HardwareAction};
use crate::robot::{RobotAction, RobotView};
use crate::robot::run::RunAction;

/// The section's actions. Motion (`SyncStart`) is refused from REST by the caller.
pub fn apply(hw: &mut Hardware, action: &HardwareAction, view: Option<&RobotView>, run: &mut dyn FnMut(RobotAction)) -> Result<(), String> {
    let s = &mut hw.sync;
    match action {
        HardwareAction::SyncConnect => return s.connect(),
        HardwareAction::SyncInspect => return s.inspect(),
        HardwareAction::SyncStart => {
            s.start(view, run)?;
            if s.preparing {
                // The page saves the mapping when a session starts.
                hw.settings.sync = hw.sync.to_save();
            }
            return Ok(());
        }
        HardwareAction::SyncStop => {
            s.stop("Operator stop");
            run(RobotAction::Run { action: RunAction::Pause });
            return Ok(());
        }
        HardwareAction::SyncLeg { leg } => {
            s.editable()?;
            if !s.legs.contains(leg) {
                return Err(format!("motor sync leg `{leg}`: the bench offers {}", s.legs.join(", ")));
            }
            s.leg = leg.clone();
            let config = s.config.as_ref().expect("editable");
            s.rows = mapping(&config.coordinates, &config.ids, leg, None);
        }
        HardwareAction::SyncMotor { row, motor_id } => {
            s.editable()?;
            if !s.motor_ids().contains(motor_id) {
                return Err(format!("motor ID {motor_id} is not on the bench (its motors: {:?})", s.motor_ids()));
            }
            let n = s.rows.len();
            s.rows.get_mut(*row).ok_or_else(|| format!("mapping row {row}: the leg has {n} rows"))?.motor_id = *motor_id;
        }
        HardwareAction::SyncPolarity { row, polarity } => {
            s.editable()?;
            if !matches!(polarity, 1 | -1) {
                return Err(format!("polarity {polarity}: expected 1 or -1"));
            }
            let n = s.rows.len();
            s.rows.get_mut(*row).ok_or_else(|| format!("mapping row {row}: the leg has {n} rows"))?.polarity = *polarity;
        }
        HardwareAction::SyncScale { scale } => {
            s.editable()?;
            if !SCALES.iter().any(|(x, _)| x == scale) {
                return Err(format!("bench motion scale {scale}: expected 0.03, 0.05 or 0.09"));
            }
            s.amplitude = *scale;
        }
        other => return Err(format!("`{}` is not a motor sync action", other.name())),
    }
    s.revision += 1;
    s.samples_revision += 1;
    hw.settings.sync = hw.sync.to_save();
    Ok(())
}
