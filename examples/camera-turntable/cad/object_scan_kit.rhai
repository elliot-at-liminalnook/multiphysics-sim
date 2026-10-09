// Object-scanning kit for the camera turntable (mm, Z up, turntable axis on Z).
//
//     POST /v1/batch {"commands": [{"command": "cad_script",
//         "args": {"path": "examples/camera-turntable/cad/object_scan_kit.rhai"}}]}
//
// For scanning objects, the object sits on a platform on the disc and the
// camera looks in from a fixed stand beside the turntable. The camera and the
// ESP32 move from the disc to the stand, so no cable winds up. Take the camera
// mount off the disc (two M3 screws).
//
// * Platform: a Ø160 mm printed plate on three standoffs above the slip-ring
//   clamp, screwed to the disc through the carrier screw circle.
// * Stand: a base plate that sits on the table (weigh it down or tape it), a
//   post, and a camera head on an M3 pivot with two detent holes. It gives a
//   low ring (15° down) and a high ring (35° down), both aimed at the platform
//   centre. Each detent has a reference camera body carrying its optics, from
//   which derive_physics exports object-rig-low.json and object-rig-high.json.
//
// In the object's frame the camera orbits the other way, which is why those
// rig files have their axis reversed. Use a plain backdrop and even, diffuse
// light: in the object's frame the room and a directional light turn, and
// matching assumes they don't.

fn defaults() {
    #{ platform_diameter: 160.0, platform_height: 84.0, stand_distance: 250.0, object_center_height: 40.0,
       low_tilt_deg: 15.0, high_tilt_deg: 35.0, stand_angle_deg: 0.0 }
}

fn build(params) {
    let p = defaults();
    p.mixin(params);
    let CAMERA = #{ w: 25.0, h: 24.0, lens_len: 11.4, mass: 0.004 };
    let M3 = 3.4;
    let made = #{};

    let top = p.platform_height;
    let z_disc_top = 58.0;
    // Platform on three standoffs over the slip-ring clamp.
    let plate = cyl(p.platform_diameter / 2.0, top - 4.0, top);
    for a in [30.0, 150.0, 270.0] {
        let x = 32.0 * cos(rad(a));
        let y = 32.0 * sin(rad(a));
        plate = union(plate, subtract(cyl(5.0, z_disc_top, top - 4.0, x, y), cyl(M3 / 2.0, z_disc_top - 1.0, top + 1.0, x, y)));
        plate = subtract(plate, cyl(3.2, top - 2.0, top + 1.0, x, y));  // screw-head counterbore
    }
    made.platform = add_body(plate, "Object platform (on the disc)", "pla", [0.95, 0.95, 0.93],
        #{ "print": #{ role: "object platform", material: "PLA", status: "designed, not yet printed",
                     notes: "Three M3×30 through the standoffs into the carrier screw circle. Print white or matte; a textured top helps matching." } });

    // Stand: base, post, and two detent camera positions aimed at the object centre.
    let sa = rad(p.stand_angle_deg);
    let R = p.stand_distance;
    let target = [0.0, 0.0, top + p.object_center_height];
    let base = box(R - 50.0, -40.0, 0.0, R + 50.0, 40.0, 6.0);
    let post_h = 0.0;
    let cams = [];
    for ring in [["low", p.low_tilt_deg], ["high", p.high_tilt_deg]] {
        // Camera on the line from the target, `tilt` below horizontal, at the stand distance horizontally.
        let tilt = ring[1];
        let t = rad(tilt);
        let horizontal = R - 30.0;  // lens front sits 30 mm in front of the post
        let z = target[2] + horizontal * tan(t);
        cams.push(#{ ring: ring[0], center: [horizontal, 0.0, z], axis: [-cos(t), 0.0, -sin(t)], tilt: tilt });
        post_h = max(post_h, z + 20.0);
    }
    let post = box(R - 8.0, -8.0, 6.0, R + 8.0, 8.0, post_h);
    for c in cams {
        post = subtract(post, cylinder([R - 9.0, 0.0, c.center[2]], [1.0, 0.0, 0.0], M3 / 2.0, 18.0));  // detent hole
    }
    let stand = union(base, post);
    made.stand = add_body(stand, "Camera stand (fixed, beside the turntable)", "pla", [0.2, 0.46, 0.72],
        #{ "print": #{ role: "camera stand", material: "PLA", status: "designed, not yet printed",
                     notes: "Two detent holes: low ring and high ring. The camera head pivots on an M3 bolt; the ESP32 board screws to the base plate." } });
    for c in cams {
        // Reference camera body at this detent: board behind the lens, lens facing the target.
        let cx = c.center[0];
        let cz = c.center[2];
        let body = box(cx, -CAMERA.w / 2.0, cz - CAMERA.h / 2.0, cx + 1.0, CAMERA.w / 2.0, cz + CAMERA.h / 2.0);
        body = union(body, cylinder([cx, 0.0, cz], [-1.0, 0.0, 0.0], 6.0, CAMERA.lens_len));
        body = rotate(body, [0.0, 1.0, 0.0], c.tilt, [cx, 0.0, cz]);
        let lens = [cx + c.axis[0] * CAMERA.lens_len, 0.0, cz + c.axis[2] * CAMERA.lens_len];
        made[`camera_${c.ring}`] = add_body(body, `Object camera (${c.ring} ring, reference)`, "pcb", [0.1, 0.42, 0.24],
            #{ purchased: #{ item: "Raspberry Pi Camera Module 3 Wide (the one from the disc)", status: "owned" },
               optics: #{ optical_center_mm: [lens[0] * cos(sa), lens[0] * sin(sa), lens[2]],
                          optical_axis: [c.axis[0] * cos(sa), c.axis[0] * sin(sa), c.axis[2]],
                          ring: c.ring, tilt_down_deg: c.tilt, aim_mm: target },
               measure_before_print: "tilt of each detent after assembly (the rig file assumes the nominal angle)" });
    }
    if p.stand_angle_deg != 0.0 {
        op("transform", [[made.stand, made.camera_low, made.camera_high]], #{ axis: [0.0, 0.0, 1.0], angle_deg: p.stand_angle_deg, center: [0.0, 0.0, 0.0] });
    }
    let group = op("group", [made.values()], #{ name: "Object-scanning kit" });
    let cameras = #{};
    for c in cams {
        cameras[c.ring] = #{ center_mm: c.center, axis: c.axis, tilt_deg: c.tilt };
    }
    #{ nodes: made, group: group, target_mm: target, cameras: cameras }
}
