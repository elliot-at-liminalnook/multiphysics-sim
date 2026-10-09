// Camera turntable: parametric CAD model (mm, Z up, turntable axis on Z).
//
// Run it in CAD mode (one undo step; re-running replaces the last run):
//
//     POST /v1/batch {"commands": [{"command": "cad_script",
//         "args": {"path": "examples/camera-turntable/cad/turntable_model.rhai"}}]}
//
// (or Experiments ▸ Model script with this path).
//
// A printed disc turns on two 6808-2RS bearings around a fixed printed post.
// A GT2 ring (printed into the underside of the disc) is driven by a 30-tooth
// printed pulley on the HX-30HM servo through a stock 610 mm closed GT2 loop;
// the servo cradle slides radially to tension the belt. An Adafruit 736 slip
// ring (22 mm, 6 wires, 2 A) through the post carries 5 V to the ESP32-P4
// board on the disc; the Camera Module 3 Wide looks outward from the rim.
//
// Purchased parts are reference bodies with declared masses. Dimensions not
// confirmed by a datasheet are marked `measure before print` in their
// metadata (servo shaft position and horn, slip-ring body length, ESP32
// board outline and holes). Printed parts carry print intent; their masses
// come from CAD volume × PLA density (solid), which over-states a
// sparse-infill print.

fn defaults() {
    #{
        ring_teeth: 280, pulley_teeth: 30, belt_length: 610.0, belt_width: 6.0,
        disc_diameter: 300.0, disc_thickness: 4.0, base_diameter: 300.0, base_thickness: 5.0,
        camera_angle_deg: 90.0, servo_angle_deg: 0.0,
    }
}

// Purchased parts (nominal datasheet values unless marked).
fn bearing() { #{ bore: 40.0, od: 52.0, width: 7.0, mass: 0.025, item: "6808-2RS deep-groove ball bearing (40×52×7), ×2" } }
fn slip_ring() {
    #{ body_d: 22.0, body_len: 25.0, flange_d: 44.0, flange_t: 3.0, mass: 0.015,
       item: "Adafruit 736 slip ring with flange, 22 mm, 6 wires, 2 A (SRC022A-6)" }
}
fn servo_spec() {
    #{ length: 45.2, width: 24.7, height: 35.0, shaft_from_end: 12.35, horn_d: 20.0, horn_t: 3.0,
       mass: 0.052, item: "Hiwonder HX-30HM serial bus servo" }
}
fn camera_spec() {
    #{ w: 25.0, h: 24.0, board_t: 1.0, lens_d: 12.0, lens_len: 11.4,
       holes: [[2.0, 2.0], [23.0, 2.0], [2.0, 14.5], [23.0, 14.5]], lens_center: [12.5, 14.5], mass: 0.004,
       item: "Raspberry Pi Camera Module 3 Wide (IMX708, 102° H FoV)" }
}
fn board_spec() {
    #{ l: 65.0, w: 30.0, t: 1.6, holes: [[3.5, 3.5], [61.5, 3.5], [3.5, 26.5], [61.5, 26.5]], mass: 0.015,
       item: "ESP32-P4-WIFI6-M development board (Waveshare)" }
}

// ---------------------------------------------------------------- helpers

fn merge(a, b) { let m = a; m.mixin(b); m }

fn printed(role, notes) {
    #{ "print": #{ role: role, material: "PLA", status: "designed, not yet printed", notes: notes } }
}

fn bought(spec, status) { #{ purchased: #{ item: spec.item, status: status } } }

// Python's `x % 360` (never negative) less 180: the signed angle in (-180, 180].
fn wrap180(x) {
    let m = (x + 180.0) % 360.0;
    if m < 0.0 { m += 360.0; }
    m - 180.0
}

// A box in the servo frame (radial, tangential) → world, turned to the servo angle.
fn servo_box(c, angle, x_rad, y_tan, z0, z1) {
    rotate_z(translate(box(x_rad[0], y_tan[0], z0, x_rad[1], y_tan[1], z1), [c, 0.0, 0.0]), angle)
}

// A vertical slot (stadium) cut from z = -1 upward by `height`.
fn slot_cut(a, b, width, height) {
    sketch_extrude(#{ origin: [0.0, 0.0, -1.0], normal: [0.0, 0.0, 1.0], x_axis: [1.0, 0.0, 0.0] }, [["slot", [a, b, width]]], [0.0, 0.0, 1.0], height)
}

fn polar(r, deg_) { [r * cos(rad(deg_)), r * sin(rad(deg_))] }

fn path_at(c, ring_r, pulley_r) {
    belt_path([#{ center: [0.0, 0.0], radius: ring_r }, #{ center: [c, 0.0], radius: pulley_r }])
}

fn build(params) {
    let p = defaults();
    p.mixin(params);
    let made = #{};
    let BEARING = bearing();
    let SLIP = slip_ring();
    let SERVO = servo_spec();
    let CAMERA = camera_spec();
    let BOARD = board_spec();
    let M3_CLEAR = 3.4;
    let M3_TAP = 2.5;
    let PLA_A = [0.93, 0.55, 0.16];
    let PLA_B = [0.2, 0.46, 0.72];
    let METAL = [0.72, 0.74, 0.78];
    let DARK = [0.16, 0.17, 0.19];
    let PCB = [0.1, 0.42, 0.24];

    // ------------------------------------------------------------ belt layout
    let ring_r = gt2_pitch_radius(p.ring_teeth);
    let pulley_r = gt2_pitch_radius(p.pulley_teeth);
    let lo = ring_r + pulley_r + 4.0;
    let hi = 200.0;
    for step in 0..80 {  // centre distance that fits the stock belt exactly
        let mid = (lo + hi) / 2.0;
        if path_at(mid, ring_r, pulley_r).length < p.belt_length { lo = mid; } else { hi = mid; }
    }
    let C = (lo + hi) / 2.0;
    let sa = rad(p.servo_angle_deg);
    let sx = C * cos(sa);
    let sy = C * sin(sa);

    // ------------------------------------------------------------ heights
    let zb = p.base_thickness;                  // base top
    let z_servo_top = zb + SERVO.height;
    let z_horn_top = z_servo_top + SERVO.horn_t;
    let z_belt0 = z_horn_top + 1.0;             // pulley lower flange 1 mm
    let z_belt1 = z_belt0 + p.belt_width + 1.0; // teeth band (belt + 1 mm)
    let z_disc0 = z_belt1 + 1.0 + 2.0;          // upper flange, then 2 mm clearance
    let z_disc1 = z_disc0 + p.disc_thickness;
    let bw = BEARING.width;
    let z_bear_lo = 12.0;
    let z_bear_hi = z_disc0 - bw;
    let post_r = BEARING.bore / 2.0;
    let bore_r = SLIP.body_d / 2.0 + 0.5;
    let hub_r = BEARING.od / 2.0 + 0.05;

    // ------------------------------------------------------------ base + post (one print)
    let base = cyl(p.base_diameter / 2.0, 0.0, zb);
    base = union(base, [cyl(post_r + 1.5, zb, z_bear_lo), cyl(post_r - 0.05, z_bear_lo, z_disc0)]);
    base = subtract(base, cyl(bore_r, -1.0, z_disc0 + 1.0));
    base = subtract(base, box(0.0, -3.0, -1.0, -p.base_diameter / 2.0 - 1.0, 3.0, 2.5));       // underside cable channel
    base = subtract(base, box(-p.base_diameter / 2.0 + 8.0, -6.0, -1.0, -p.base_diameter / 2.0 + 11.0, 6.0, zb + 1.0));  // zip-tie slot
    for a in [0.0, 120.0, 240.0] {                                                             // cap screws
        let at = polar(post_r - 4.0, a + 30.0);
        base = subtract(base, cyl(M3_TAP / 2.0, z_disc0 - 10.0, z_disc0 + 1.0, at[0], at[1]));
    }
    // Servo cradle slots: two M4 slots along the radius for belt tension.
    for t in [-8.0, 26.0] {
        let cx = sx + t * -sin(sa);
        let cy = sy + t * cos(sa);
        base = subtract(base, slot_cut([cx - 5.0 * cos(sa), cy - 5.0 * sin(sa)], [cx + 5.0 * cos(sa), cy + 5.0 * sin(sa)], 4.4, zb + 2.0));
    }
    for a in [45.0, 135.0, 225.0, 315.0] {                                                     // lightening windows
        if abs(wrap180(a - p.servo_angle_deg)) < 40.0 { continue; }
        base = subtract(base, slot_cut(polar(60.0, a), polar(115.0, a), 30.0, zb + 2.0));
    }
    let meta = printed("base", "Print flat. Post outer Ø39.9 is a light press for the 6808 inner races; tune to your printer with the fit coupon.");
    meta["print"]["ground"] = true;
    made.base = add_body(base, "Base and bearing post", "pla", PLA_B, meta);

    let cap = subtract(cyl(post_r + 1.5, z_disc0, z_disc0 + 3.0), cyl(bore_r, z_disc0 - 1.0, z_disc0 + 4.0));
    for a in [0.0, 120.0, 240.0] {
        let at = polar(post_r - 4.0, a + 30.0);
        cap = subtract(cap, cyl(M3_CLEAR / 2.0, z_disc0 - 1.0, z_disc0 + 4.0, at[0], at[1]));
    }
    made.cap = add_body(cap, "Bearing retainer cap", "pla", PLA_B, printed("retainer", "Clamps the upper inner race; 3 × M3×8 into the post."));

    // ------------------------------------------------------------ servo, cradle, pulley
    let L = SERVO.length;
    let W = SERVO.width;
    let H = SERVO.height;
    let e = SERVO.shaft_from_end;
    let ang = p.servo_angle_deg;
    // Servo long axis tangential (+t), shaft end first, so the body stays inside the disc radius.
    let servo = servo_box(C, ang, [-W / 2.0, W / 2.0], [-e, L - e], zb, z_servo_top);
    servo = union(servo, rotate_z(cyl(3.0, z_servo_top, z_servo_top + 1.0, C, 0.0), ang));
    let m = merge(bought(SERVO, "owned"), declared(servo, SERVO.mass, "manufacturer mass 52 g; uniform density over the envelope"));
    m.measure_before_print = "shaft position along the case (12.35 mm from the end is an estimate) and case size";
    made.servo = add_body(servo, "HX-30HM servo (reference)", "abs", DARK, m);
    let horn = rotate_z(cyl(SERVO.horn_d / 2.0, z_servo_top, z_horn_top, C, 0.0), ang);
    let m = merge(#{ purchased: #{ item: "HX-30HM round metal horn (supplied with the servo)", status: "owned" } }, declared(horn, 0.004, "estimate"));
    m.measure_before_print = "horn diameter, thickness and screw circle";
    made.horn = add_body(horn, "Servo horn (reference)", "al", METAL, m);

    let cl = 0.4;
    let wall = 3.0;
    let cradle = servo_box(C, ang, [-W / 2.0 - cl - wall, W / 2.0 + cl + wall], [-e - cl - wall, L - e + cl + wall], zb, zb + 22.0);
    cradle = subtract(cradle, servo_box(C, ang, [-W / 2.0 - cl, W / 2.0 + cl], [-e - cl, L - e + cl], zb + 3.0, zb + 30.0));
    cradle = union(cradle, servo_box(C, ang, [-W / 2.0 - cl - wall - 6.0, W / 2.0 + cl + wall + 6.0], [-e - cl - wall, L - e + cl + wall], zb, zb + 3.0));
    for t in [-8.0, 26.0] {  // M4 through the slots in the base
        cradle = subtract(cradle, rotate_z(cyl(2.2, zb - 1.0, zb + 4.0, C, t), ang));
    }
    cradle = subtract(cradle, servo_box(C, ang, [-5.0, 5.0], [L - e - 2.0, L - e + cl + wall + 1.0], zb + 6.0, zb + 30.0));  // cable exit
    let m = printed("servo cradle", "Two M4 bolts through the base slots; slide outward to tension the belt, then tighten.");
    m["print"]["clearance_mm"] = cl;
    made.cradle = add_body(cradle, "Servo cradle (slides to tension the belt)", "pla", PLA_A, m);

    let pulley = prism(gt2_outline(p.pulley_teeth), z_belt0, z_belt1);
    let fl = gt2_tip_radius(p.pulley_teeth) + 2.0;
    pulley = union(pulley, [cyl(fl, z_horn_top, z_belt0), cyl(fl, z_belt1, z_belt1 + 1.0)]);
    pulley = translate(pulley, [C, 0.0, 0.0]);
    pulley = subtract(pulley, cyl(1.6, z_horn_top - 1.0, z_belt1 + 2.0, C, 0.0));
    for a in [45.0, 135.0, 225.0, 315.0] {
        let at = polar(7.0, a);
        pulley = subtract(pulley, cyl(M3_CLEAR / 2.0 - 0.3, z_horn_top - 1.0, z_belt1 + 2.0, C + at[0], at[1]));
    }
    pulley = rotate_z(pulley, ang);
    let m = printed("drive pulley", "Screws to the servo horn (4 × M2.5/M3 on a 14 mm circle: check your horn).");
    m.belt_pulley = #{ drive: "turntable", order: 1, teeth: p.pulley_teeth, side: "teeth", role: "driver", pretension_n: 25.0,
        pretension_uncertainty: 0.5, pretension_provenance: "estimated",
        pretension_source: "Typical static tension per run for a 6 mm GT2 belt, set by sliding the servo cradle; measure with a belt tension gauge or the pluck frequency" };
    made.pulley = add_body(pulley, `GT2 ${p.pulley_teeth}T drive pulley`, "pla", PLA_A, m);

    // ------------------------------------------------------------ disc + ring + hub (one print, upside down)
    let R_disc = p.disc_diameter / 2.0;
    let disc = cyl(R_disc, z_disc0, z_disc1);
    let ring = prism(gt2_outline(p.ring_teeth), z_belt0, z_disc0);
    ring = subtract(ring, cyl(gt2_tip_radius(p.ring_teeth) - 7.0, z_belt0 - 1.0, z_disc0 + 1.0));
    let hub = subtract(cyl(hub_r + 4.0, z_bear_lo, z_disc0), [cyl(hub_r, z_bear_lo - 1.0, z_bear_lo + bw), cyl(hub_r, z_bear_hi, z_disc1 + 1.0), cyl(post_r + 3.0, z_bear_lo, z_disc0 + 1.0)]);
    disc = union(disc, [ring, hub]);
    disc = subtract(disc, cyl(hub_r, z_disc0 - 1.0, z_disc1 + 1.0));
    for a in [0.0, 90.0, 180.0, 270.0] {  // carrier screws
        let at = polar(32.0, a + 45.0);
        disc = subtract(disc, cyl(M3_TAP / 2.0, z_disc0, z_disc1 + 1.0, at[0], at[1]));
    }
    for a in [0.0, 60.0, 120.0, 180.0, 240.0, 300.0] {  // spokes between the ring and the rim stay; windows lighten the web
        if abs(wrap180(a - p.camera_angle_deg)) < 50.0 { continue; }
        let plane = #{ origin: [0.0, 0.0, z_disc0 - 1.0], normal: [0.0, 0.0, 1.0], x_axis: [1.0, 0.0, 0.0] };
        disc = subtract(disc, sketch_extrude(plane, [["slot", [polar(45.0, a), polar(70.0, a), 18.0]]], [0.0, 0.0, 1.0], p.disc_thickness + 2.0));
    }
    let m = printed("disc", "Print upside down (disc face on the bed). Hub bore Ø52.1 takes both 6808 outer races; the internal rib locates them.");
    m.belt_pulley = #{ drive: "turntable", order: 0, teeth: p.ring_teeth, side: "teeth", role: "driven" };
    made.disc = add_body(disc, "Turntable disc with GT2 ring and bearing hub", "pla", PLA_A, m);

    let i = 0;
    for z0 in [z_bear_lo, z_bear_hi] {
        let b = subtract(cyl(BEARING.od / 2.0, z0, z0 + bw), cyl(BEARING.bore / 2.0, z0 - 1.0, z0 + bw + 1.0));
        let which = if i == 0 { "lower" } else { "upper" };
        made[`bearing${i}`] = add_body(b, `6808-2RS bearing (${which})`, "steel", METAL, merge(bought(BEARING, "to buy"), declared(b, BEARING.mass, "typical catalogue mass for 6808-2RS")));
        i += 1;
    }

    // ------------------------------------------------------------ slip ring
    let z_carrier = z_disc1;
    let carrier = subtract(cyl(38.0, z_carrier, z_carrier + 3.0), cyl(bore_r, z_carrier - 1.0, z_carrier + 4.0));
    for a in [0.0, 90.0, 180.0, 270.0] {
        let at = polar(32.0, a + 45.0);
        carrier = subtract(carrier, cyl(M3_CLEAR / 2.0, z_carrier - 1.0, z_carrier + 4.0, at[0], at[1]));
    }
    made.carrier = add_body(carrier, "Slip-ring carrier", "pla", PLA_B, printed("slip-ring carrier", "Bridges the disc centre; 4 × M3×8 into the disc."));
    let zf = z_carrier + 3.0;
    let clamp = subtract(cyl(30.0, zf, zf + SLIP.flange_t + 2.0), [cyl(SLIP.flange_d / 2.0 + 0.3, zf - 1.0, zf + SLIP.flange_t), cyl(SLIP.body_d / 2.0 - 2.0, zf - 1.0, zf + 10.0)]);
    for a in [0.0, 120.0, 240.0] {
        let at = polar(26.0, a);
        clamp = subtract(clamp, cyl(M3_CLEAR / 2.0, zf - 1.0, zf + 10.0, at[0], at[1]));
        carrier = subtract(carrier, cyl(M3_TAP / 2.0, z_carrier - 1.0, zf + 1.0, at[0], at[1]));
    }
    set_body(made.carrier, carrier);
    made.clamp = add_body(clamp, "Slip-ring flange clamp", "pla", PLA_B,
        printed("flange clamp", "Clamps the slip-ring flange to the carrier, so the flange hole pattern does not matter. 3 × M3×10."));
    let ring_body = union(cyl(SLIP.body_d / 2.0, zf - SLIP.body_len, zf), cyl(SLIP.flange_d / 2.0, zf, zf + SLIP.flange_t));
    let m = merge(bought(SLIP, "to buy"), declared(ring_body, SLIP.mass, "estimate: capsule slip ring of this size"));
    m.purchased.url = "https://www.adafruit.com/product/736";
    m.purchased.price_usd = 14.95;
    m.measure_before_print = "body length below the flange (25 mm assumed); the post bore is through, so only clearance depends on it";
    m.wiring = "flange side turns with the disc (to the ESP32 5 V/GND); the other side's wires run down the post and are zip-tied at the base slot, which stops them turning";
    made.slipring = add_body(ring_body, "Slip ring (reference)", "abs", METAL, m);

    // ------------------------------------------------------------ camera + ESP32 on the disc
    let ca = p.camera_angle_deg;
    let y_plate = R_disc - 10.0;
    let z_cam = z_disc1 + 26.0;
    let mount = box(-18.0, y_plate, z_disc1, 18.0, y_plate + 3.0, z_cam + 16.0);
    mount = union(mount, box(-18.0, y_plate - 22.0, z_disc1, 18.0, y_plate + 3.0, z_disc1 + 3.0));
    for gx in [-18.0, 15.0] {  // side gussets
        let plane = #{ origin: [gx, 0.0, 0.0], normal: [1.0, 0.0, 0.0], x_axis: [0.0, 1.0, 0.0] };
        mount = union(mount, sketch_extrude(plane, [["polyline", [[[y_plate - 20.0, z_disc1 + 3.0], [y_plate, z_disc1 + 3.0], [y_plate, z_cam]], true]]], [1.0, 0.0, 0.0], 3.0));
    }
    let cx0 = -CAMERA.w / 2.0;
    let cz0 = z_cam - CAMERA.lens_center[1];
    for h in CAMERA.holes {
        mount = union(mount, cylinder([cx0 + h[0], y_plate + 3.0, cz0 + h[1]], [0.0, 1.0, 0.0], 2.2, 2.0));
        mount = subtract(mount, cylinder([cx0 + h[0], y_plate - 1.0, cz0 + h[1]], [0.0, 1.0, 0.0], 0.9, 7.0));
    }
    for x in [-12.0, 12.0] {
        mount = subtract(mount, cyl(M3_CLEAR / 2.0, z_disc1 - 1.0, z_disc1 + 4.0, x, y_plate - 12.0));
        set_body(made.disc, subtract(body_of(made.disc), cyl(M3_TAP / 2.0, z_disc0, z_disc1 + 1.0, x, y_plate - 12.0)));
    }
    mount = subtract(mount, box(-8.0, y_plate - 1.0, z_disc1 + 3.0, 8.0, y_plate + 4.0, z_disc1 + 7.0));  // ribbon slot
    made.camera_mount = add_body(rotate_z(mount, ca - 90.0), "Camera mount", "pla", PLA_B,
        printed("camera mount", "Camera faces outward; M2 screws on 2 mm bosses. The ribbon passes through the slot to the ESP32."));
    let cam = box(cx0, y_plate + 5.0, cz0, cx0 + CAMERA.w, y_plate + 5.0 + CAMERA.board_t, cz0 + CAMERA.h);
    cam = union(cam, cylinder([0.0, y_plate + 5.0 + CAMERA.board_t, z_cam], [0.0, 1.0, 0.0], CAMERA.lens_d / 2.0, CAMERA.lens_len - CAMERA.board_t));
    cam = rotate_z(cam, ca - 90.0);
    let reach = y_plate + 5.0 + CAMERA.lens_len;
    let m = merge(bought(CAMERA, "owned (2, one spare)"), declared(cam, CAMERA.mass, "Raspberry Pi product brief (≈4 g)"));
    m.optics = #{ horizontal_fov_deg: 102.0, diagonal_fov_deg: 120.0, sensor: "Sony IMX708, 4608 × 2592, rolling shutter",
        optical_axis: [cos(rad(ca)), sin(rad(ca)), 0.0],
        optical_center_mm: [reach * cos(rad(ca)), reach * sin(rad(ca)), z_cam] };
    m.measure_before_print = "lens centre height on the board (14.5 mm from the bottom edge assumed)";
    made.camera = add_body(cam, "Camera Module 3 Wide (reference)", "pcb", PCB, m);

    let yb0 = 52.0;
    let parts = [];
    for h in BOARD.holes {
        parts.push(subtract(cyl(2.8, z_disc1, z_disc1 + 5.0, h[1] - BOARD.w / 2.0, yb0 + h[0]), cyl(1.1, z_disc1, z_disc1 + 6.0, h[1] - BOARD.w / 2.0, yb0 + h[0])));
    }
    let standoffs = union(parts[0], parts.extract(1));
    made.standoffs = add_body(rotate_z(standoffs, ca - 90.0), "ESP32 board standoffs", "pla", PLA_B,
        printed("standoffs", "Glue or print onto the disc; M2.5 thread-forming screws."));
    let board = box(-BOARD.w / 2.0, yb0, z_disc1 + 5.0, BOARD.w / 2.0, yb0 + BOARD.l, z_disc1 + 5.0 + BOARD.t);
    board = union(board, box(-6.0, yb0 + BOARD.l - 1.0, z_disc1 + 5.0 + BOARD.t, 6.0, yb0 + BOARD.l + 1.5, z_disc1 + 9.0));  // camera connector
    let m = merge(bought(BOARD, "owned (2, one spare)"), declared(board, BOARD.mass, "estimate"));
    m.measure_before_print = "board outline and hole pattern (Pi Zero 65 × 30 mm, holes 58 × 23 mm assumed)";
    made.board = add_body(rotate_z(board, ca - 90.0), "ESP32-P4-WIFI6-M (reference)", "pcb", PCB, m);

    // ------------------------------------------------------------ belt (reference)
    let path = path_at(C, ring_r, pulley_r);
    let pts_in = [];
    let pts_out = [];
    let circles = [[[0.0, 0.0], ring_r], [[C, 0.0], pulley_r]];
    let spans = path.spans;
    let n = circles.len();
    for i in 0..n {
        let c = circles[i][0];
        let r = circles[i][1];
        let prev = spans[(i + n - 1) % n];
        let a0 = atan2(prev.end[1] - c[1], prev.end[0] - c[0]);
        let wrap = path.wraps[i].angle;
        let steps = max(8, (wrap * r / 1.5).to_int());
        for j in 0..=steps {
            let a = a0 + wrap * j / steps;
            pts_in.push([c[0] + (r - 0.75) * cos(a), c[1] + (r - 0.75) * sin(a)]);
            pts_out.push([c[0] + (r + 0.63) * cos(a), c[1] + (r + 0.63) * sin(a)]);
        }
    }
    let belt = subtract(prism(pts_out, z_belt0 + 0.5, z_belt0 + 0.5 + p.belt_width), prism(pts_in, z_belt0, z_belt1 + 1.0));
    let length = p.belt_length.round().to_int();
    let teeth = (p.belt_length / 2.0).round().to_int();
    let m = merge(#{ purchased: #{ item: `GT2 closed-loop belt, ${length} mm (${teeth} teeth), 6 mm wide, glass-fibre`, status: "to buy" } },
        declared(belt, 0.007 * p.belt_length / 1000.0, "about 7 g/m for 6 mm GT2"));
    made.belt = add_body(rotate_z(belt, ang), `GT2 closed belt ${length} mm (reference)`, "rubber", DARK, m);

    // ------------------------------------------------------------ groups, joints
    let fixed = ["base", "cap", "cradle", "servo", "horn", "pulley", "bearing0", "bearing1", "belt"].map(|k| made[k]);
    let turning = ["disc", "carrier", "clamp", "slipring", "camera_mount", "camera", "standoffs", "board"].map(|k| made[k]);
    let g_fixed = op("group", [fixed], #{ name: "Base (fixed)" });
    let g_turn = op("group", [turning], #{ name: "Turntable (rotating)" });
    op("set_ground", [made.base]);
    for nid in fixed.extract(1) {
        if nid != made.pulley && nid != made.horn && nid != made.belt {
            op("connect_fixed", [made.base, nid]);
        }
    }
    for nid in turning.extract(1) {
        op("connect_fixed", [made.disc, nid]);
    }
    let joint = op("add_joint", ["continuous", made.base, made.disc, [0.0, 0.0, z_disc0], [0.0, 0.0, 1.0]], #{ name: "Turntable axis" });
    let wrap = path.wraps[1].angle;
    #{
        nodes: made, groups: #{ fixed: g_fixed, turning: g_turn }, joint: joint,
        layout: #{
            center_distance_mm: C, ratio: ring_r / pulley_r, belt_length_mm: path.length,
            driver_wrap_deg: deg(wrap), teeth_in_mesh: p.pulley_teeth * wrap / (2.0 * PI()),
            z_belt_center_mm: (z_belt0 + z_belt1) / 2.0, z_disc_top_mm: z_disc1, camera_height_mm: z_cam,
        },
    }
}
