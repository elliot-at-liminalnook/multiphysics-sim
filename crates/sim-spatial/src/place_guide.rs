//! `place_guide` (and `GET /v1/place_guide`): how Place mode works, for an
//! agent starting cold. Concepts, workflows in order, every command with a
//! working example, and the rules. Every command's full description and
//! refusals are in `GET /v1/capabilities`.
use serde_json::{Value, json};

/// The guide (`topic` narrows it to one section).
pub(crate) fn guide(topic: Option<&str>) -> Result<Value, String> {
    let all = json!({
        "about": "Place mode is a walkthrough of a scanned place: the fused, coloured mesh that sim-place build made from turntable scans, with a marker at every scan station (blue posts) and every photo (red dots, with a stick along the viewing direction). It is presentation only: nothing here changes the place model.",
        "how_to_call": {
            "batch": "POST /v1/batch {\"commands\":[{\"command\":NAME,\"args\":{...}}]} → {\"job_id\"}; GET /v1/jobs/{job_id} until it ends; results[i].ok / value / error. Commands in one batch run in order.",
            "resources": "GET /v1/place_guide (this), /v1/capabilities (every command), /v1/state (the same answer as state, refreshed as the view changes).",
            "entering": "viewer_mode {mode: place, path: /abs/place-dir} (a sim-place build directory holding place.json), or launch sim-spatial DIR.",
            "seeing": "screenshot {path} saves the window as drawn, from the camera as it stands; aim it first with camera. For off-screen renders and geometric questions use the sim-place tools on the same directory (sim-place query DIR TOOL ARGS, or sim-place mcp DIR as an MCP server): place_overview, render_view, list_photos, get_photo, photos_of_point, raycast, measure, surfaces, height_at, clearance, free_space_map, plan_path.",
        },
        "concepts": {
            "frames": "Everything here is in the viewer frame: metres, +Y up. The place files and the sim-place tools use the place frame, +Z up. A place point (x, y, z) is the viewer point (x, z, −y); a viewer point (X, Y, Z) is the place point (X, −Z, Y).",
            "camera": "A first-person fly camera: position, yaw, pitch and speed. Yaw 0 looks along +X and positive yaw turns toward −Z; pitch tilts the view up (positive) or down, within ±1.5 rad. Speed is what W/A/S/D use (metres per second).",
            "stations_and_photos": "state.stations are where the scanner stood; state.photo_views are the photos, each a position and a unit viewing direction. camera {station: i} stands over a station as keys 1–9 do; camera {view: i} stands where photo i was taken, looking where it looked.",
            "markers_and_help": "The P key shows or hides the photo and station markers and H the help line; state reports both (markers_visible, help_visible).",
        },
        "workflows": [
            {"goal": "Get oriented", "steps": [
                "state: the description, how many stations and photos, where the camera is",
                "camera {station: 0}, then screenshot {path: /tmp/station0.png}",
                "repeat for the other stations, or turn with camera {yaw} between screenshots"]},
            {"goal": "See what a photo saw", "steps": [
                "state.photo_views lists every photo's position and direction",
                "camera {view: i} puts the camera at photo i, looking along it",
                "screenshot {path}; compare with sim-place query DIR get_photo for the photo itself"]},
            {"goal": "Answer a geometric question", "steps": [
                "use sim-place query (or its MCP server) for measure, raycast, clearance, height_at or plan_path; convert its +Z-up points with the frames rule",
                "camera {position: [x, y, z]} with the converted point to look at the answer in the view"]},
        ],
        "commands": {
            "place_guide": {"example": {"topic": "concepts"}, "does": "This guide; topic narrows it."},
            "state": {"example": {}, "does": "Directory, description, stations, photo views, the station last jumped to, the camera, marker and help visibility."},
            "camera": {"example": {"view": 0}, "does": "Set the fly camera: any of position, yaw, pitch, speed, and a station or a photo view to jump to first."},
        },
        "rules": [
            "Read state before aiming the camera: indices and positions come from it.",
            "Give a station or a view, not both; position, yaw and pitch given with either override the jump.",
            "Convert between the viewer frame (+Y up) and the place frame (+Z up) every time a point crosses between this mode and the sim-place tools.",
            "This mode never changes the place; rebuild it with sim-place build.",
        ],
    });
    match topic {
        None => Ok(all),
        Some(t) => all.get(t).cloned().map(|v| json!({t: v})).ok_or_else(|| format!("no guide topic {t} (about, how_to_call, concepts, workflows, commands, rules)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::actions::Action;
    use crate::place_view::PlaceAction;

    /// Every place command is in the guide, every guide command exists,
    /// and every example parses as the command it documents.
    #[test]
    fn the_guide_lists_every_place_command() {
        let all = guide(None).unwrap();
        let listed: std::collections::BTreeSet<String> = all["commands"].as_object().unwrap().keys().cloned().collect();
        let specs: std::collections::BTreeSet<String> = PlaceAction::commands().into_iter().map(|s| s.name.to_string()).collect();
        assert_eq!(listed, specs);
        for (name, c) in all["commands"].as_object().unwrap() {
            let command = sim_api::Command { command: name.clone(), args: c["example"].clone() };
            assert!(PlaceAction::parse(&command).is_ok(), "{name}: {:?}", PlaceAction::parse(&command).err());
        }
        assert_eq!(guide(Some("rules")).unwrap()["rules"], all["rules"]);
        assert!(guide(Some("nope")).is_err());
    }
}
