//! The catalogue's entries: RoboCAD's Print menu (cad-print; ui/app.py:358-359
//! and :390-400, the handlers at :889-908 and :1113-1263, `FastenerDialog`
//! ui/widgets.py:988-1021, `FastenerTool` ui/tools.py:1120-1155), then the
//! REST-only `print_split` Ops method. Labels, prompts, defaults, ranges,
//! hints and refusals are RoboCAD's; the arguments, reads and jobs are
//! built by `crate::cad::print` (`Shape::Print`). Not here: "Toggle
//! overhang shading" (a display toggle), "Strength overlay on/off" (the
//! results overlay) and "Print jobs…" (the jobs panel): `print::command_action`.
use super::super::kinds::*;
use super::super::*;
use crate::cad::print::PrintCall;
use crate::ui_kit::form::Unit;
use sim_runtime::cad_client::{FASTENER_KINDS, FASTENER_SIZES, SPLIT_JOINTS};

/// "Printer:" for a split: the registry's printers with their usable size
/// (`print::picks` "printers": "bambu-h2c (325 × 320 × 320 mm)"), the
/// first preselected (`QInputDialog.getItem(…, 0, False)`).
const SPLIT_PRINTER: Param = p("printer", "Printer:", pick("printers"), "");
/// "Joints:" (ui/app.py:1176).
const SPLIT_JOINT: Param = p("joint", "Joints:", FieldKind::Choice { options: &SPLIT_JOINTS }, "auto");

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    OpEntry {
        id: "tool.fastener",
        label: "Fastener hole…",
        category: "Print",
        keys: &["Ctrl+H"],
        needs: FACES,
        params: &[
            p("size", "Size", FieldKind::Choice { options: &FASTENER_SIZES }, "M3"),
            p("kind", "Kind", FieldKind::Choice { options: &FASTENER_KINDS }, "clearance"),
            p("extra", "Extra clearance (mm)", number(Unit::Length, 0.0, 1.0, 2), "0"),
            p("depth", "Depth (mm; 0: through)", number(Unit::Length, 0.0, 500.0, 2), "0"),
            p("point", "Point (mm; a face click sets it)", POINT, ""),
        ],
        flow: Flow::PrintPick,
        route: "fastener_hole",
        shape: Shape::Print(PrintCall::Fastener),
        refusal: "Click a face to place a hole",
        hint: "Click a face to place a hole: size and kind from the panel (M2–M8; clearance, tap, counterbore, countersink, heat-set insert)",
        source: "ui/app.py:358, ui/app.py:889-894 (FastenerDialog with the last values, remembered as last_fastener; then FastenerTool), ui/widgets.py:988-1021 (Size M2–M8 default M3, Kind clearance/tap/counterbore/countersink/insert, Extra clearance 0..1 mm in 0.05 steps, Depth 0..500 mm with 0 shown as \"through\"), ui/tools.py:1120-1155 (face mode; each face click is one Ops.fastener_hole(node, face, point, spec) at the vertex, midpoint, centre or endpoint snap, else the hit point; the tool stays active), commands.py:922; the point field and the refusal are ours (a click fills the point; REST names the face in items)",
        ..BASE
    },
    OpEntry {
        id: "tool.clearance",
        label: "Clearance offset…",
        category: "Print",
        keys: &["Ctrl+Shift+C"],
        needs: FACES,
        params: &[p("amount", "Grow holes / shrink bosses by (mm):", number(Unit::Length, -5.0, 5.0, 2), "0.2")],
        flow: Flow::Form,
        route: "clearance",
        shape: Shape::Print(PrintCall::Clearance),
        fan: Fan::PerNode,
        refusal: "Select holes, bosses or faces to offset",
        source: "ui/app.py:359, ui/app.py:896-908 (getDouble(\"Clearance\", …, ops.last_clearance, -5, 5, 2): the last amount, 0.2 at first (commands.py:271); one Ops.clearance(node, faces, amount) per node with selected faces, each its own undo step \"Clearance\"), commands.py:903",
        ..BASE
    },
    OpEntry {
        id: "print.wall_check",
        label: "Wall thickness check…",
        category: "Print",
        keys: &["Ctrl+W"],
        params: &[p("threshold", "Flag walls thinner than (mm):", number(Unit::Length, 0.1, 20.0, 2), "1.2")],
        flow: Flow::Form,
        route: "thin",
        shape: Shape::Print(PrintCall::WallCheck),
        source: "ui/app.py:390, ui/app.py:1113-1127 (getDouble(\"Wall thickness\", \"Flag walls thinner than (mm):\", 1.2, 0.1, 20, 2); the selected nodes, else every visible body; red points; \"N thin region(s) under T mm\" or \"No walls thinner than T mm\"), api.py:710-712 and :1454-1455 (GET /nodes/{id}/thin?threshold=), printing.py:98 (wall_thickness); remembering the last threshold is ours (RoboCAD's dialog opens at 1.2 each time)",
        ..BASE
    },
    OpEntry {
        id: "print.validate",
        label: "Validate for printing",
        category: "Print",
        keys: &["Ctrl+Shift+V"],
        route: "validate",
        shape: Shape::Print(PrintCall::Validate),
        source: "ui/app.py:391, ui/app.py:1129-1135 (every visible body through validate_for_export: \"N body(ies): valid and watertight.\" or one line per issue \"name: message near (x, y, z) — fix\"), printing.py:138-154, api.py:700-703 and :1450-1451 (GET /nodes/{id}/validate: the kernel's report; the tessellation's open-edge count is not served)",
        ..BASE
    },
    OpEntry {
        id: "print.split",
        label: "Split selected for printing…",
        category: "Print",
        needs: nodes(1, Some(1), &["body", "sheet"]),
        params: &[SPLIT_PRINTER, SPLIT_JOINT],
        flow: Flow::Form,
        route: "print/split",
        shape: Shape::Print(PrintCall::Split),
        refusal: "Select one body to split.",
        source: "ui/app.py:393, ui/app.py:1166-1183 (one selected body, else \"Select one body to split.\"; \"Printer:\" the registry's printers with their usable size, \"Joints:\" auto / pins+screws / dovetail / pins; PrintJobs.split_job; done: \"split into N pieces; hardware: …\"), api.py:308-309, print_jobs.py:155-184",
        ..BASE
    },
    OpEntry {
        id: "print.strength",
        label: "Check strength (document's print study)",
        category: "Print",
        route: "print/analyze",
        shape: Shape::Print(PrintCall::Strength),
        source: "ui/app.py:394, ui/app.py:1185-1200 (the document's robot_settings[\"print_study\"] as the /print/analyze body, else RoboCAD's explanation; done: \"strength: least safety factor F on NAME (MODE); Print ▸ Strength overlay shows where\"), print_jobs.py:213-245",
        ..BASE
    },
    OpEntry {
        id: "print.plan",
        label: "Plan print settings and plates (document's print study)",
        category: "Print",
        route: "print/plan",
        shape: Shape::Print(PrintCall::Plan),
        source: "ui/app.py:395, ui/app.py:1202-1211 (the print study as the /print/plan body; without one, Check strength's explanation; done: \"plan: N plate(s), about H h and G g (estimates); 3MF files in DIR\"), print_jobs.py:249-293",
        ..BASE
    },
    OpEntry {
        id: "print.strength_split",
        label: "Whole or split for strength? (selected part of the print study)",
        category: "Print",
        route: "print/strength_split",
        shape: Shape::Print(PrintCall::StrengthSplit),
        refusal: "Select a part that is in the document's print study (it needs its fixtures and loads).",
        source: "ui/app.py:396, ui/app.py:1213-1225 (the first study part whose node is selected; its printer, material, simulation, safety_target and space with node and part; done: \"RECOMMENDATION: WHY\"), print_jobs.py:296-317",
        ..BASE
    },
    OpEntry {
        id: "print.assembly",
        label: "Assembly guide for the selected split…",
        category: "Print",
        route: "print/assembly",
        shape: Shape::Print(PrintCall::Assembly),
        refusal: "Select a split (the group Split for printing made) or one of its pieces.",
        source: "ui/app.py:397, ui/app.py:1227-1242 (a selected split group, else the parent of a selected piece; done: the guide opens, \"assembly: N steps; guide PATH\"), print_jobs.py:320-342",
        ..BASE
    },
    OpEntry {
        id: "print.coupons",
        label: "Test coupons (for the selected split, or the material)…",
        category: "Print",
        params: &[p("printer", "Printer:", pick("printer_ids"), ""), p("material", "Filament:", pick("filaments"), "")],
        flow: Flow::Form,
        route: "print/coupons",
        shape: Shape::Print(PrintCall::Coupons),
        source: "ui/app.py:398, ui/app.py:1244-1263 (the selected split group or a piece's, else none: material bars only; \"Printer:\" and \"Filament:\" the registry's ids; done: the protocol's folder opens, \"coupons: N on P plate(s); …\"), print_jobs.py:345-363",
        ..BASE
    },
    // ---- REST-only print Ops method (no RoboCAD command) ------------------------
    OpEntry {
        id: "ops.print_split",
        label: "Split for printing (one undo step, no job)",
        category: "Print",
        needs: nodes(1, Some(1), &["body", "sheet"]),
        params: &[p("printer", "Printer", pick("printer_ids"), "bambu-h2c"), SPLIT_JOINT],
        flow: Flow::Form,
        route: "print_split",
        args: &[Arg::Node],
        kwargs: &[("printer", Arg::Param("printer")), ("joint", Arg::Param("joint"))],
        refusal: "Select one body to split.",
        source: "commands.py:299 (Ops.print_split(node_id, **SplitOptions): the pieces under a new group, one undo step; result: the group id), print_split.py:40-56 (SplitOptions: printer bambu-h2c, joint auto); label ours",
        ..BASE
    },
];
