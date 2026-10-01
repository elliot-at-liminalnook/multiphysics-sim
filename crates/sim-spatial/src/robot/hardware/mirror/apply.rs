//! The Simulated leg mirror section's actions ([`apply`]).
use super::{JOINTS, LEGS, Mirror};
use crate::robot::hardware::settings::MirrorBinding;
use crate::robot::hardware::{Hardware, HardwareAction};

/// The mirror's actions (the section's controls): each saves the
/// preferences, then begins again (or ends, for "off"), as :32-38.
pub fn apply(hw: &mut Hardware, action: &HardwareAction) -> Result<(), String> {
    let m = &mut hw.mirror;
    match action {
        HardwareAction::MirrorEnabled { on } => {
            m.settings.enabled = *on;
            if *on { m.begin() } else { m.end() }
        }
        HardwareAction::MirrorLeg { leg } => {
            if !LEGS.contains(&leg.as_str()) {
                return Err(format!("mirror leg `{leg}`: expected one of {}", LEGS.join(", ")));
            }
            m.settings.leg = leg.clone();
            m.begin();
        }
        HardwareAction::MirrorJoint { id, joint } => {
            if !JOINTS.iter().any(|(j, _)| j == joint) {
                return Err(format!("mirror joint `{joint}`: expected one of {}", JOINTS.map(|(j, _)| j).join(", ")));
            }
            binding(m, *id)?.joint = joint.clone();
            m.begin();
        }
        HardwareAction::MirrorPolarity { id, polarity } => {
            if !matches!(polarity, 1 | -1) {
                return Err(format!("mirror sign {polarity}: expected 1 or -1"));
            }
            binding(m, *id)?.polarity = *polarity;
            m.begin();
        }
        HardwareAction::MirrorAlign { id, align } => {
            binding(m, *id)?.align = *align;
            m.begin();
        }
        other => return Err(format!("`{}` is not a mirror action", other.name())),
    }
    m.revision += 1;
    hw.settings.mirror = hw.mirror.to_save();
    hw.settings.save();
    Ok(())
}

fn binding(m: &mut Mirror, id: u8) -> Result<&mut MirrorBinding, String> {
    let listed = m.roles.is_some();
    m.settings.bindings.get_mut(&id).ok_or_else(|| {
        if listed { format!("motor ID {id} is not on the calibration server's list") } else { "the calibration server has not listed its motors yet; connect first".to_string() }
    })
}
