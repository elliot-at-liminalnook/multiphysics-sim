//! Exactness checks against RoboCAD: its comment-pin stamps (computed by
//! cad/robocad's `annotations.stamp` with OCCT 7.7.2 on the rover baseline)
//! and the hidden-line views' orientation.
use sim_cad::kernel::{self, Op, Shape};

#[test]
fn pin_stamps_match_robocads_byte_for_byte() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/wheeled-robot/baseline/robot.rcad");
    let doc = sim_cad::ArchiveDocument::open(&root).unwrap();
    // (node, as drawn, as read): RoboCAD 2026-10-03, `stamp` after and before `tessellate`.
    let expected = [
        ("268f56a63e3f", "bd31c3e1f8487c437e4396bdedab0ddaa1a08cc8bb8d3612b665cc2e97f25ddd", "bd31c3e1f8487c437e4396bdedab0ddaa1a08cc8bb8d3612b665cc2e97f25ddd"),
        ("1d090f0c7227", "75ad05f0f1065ae1f38fad90b8fdfed1d49fc1d625e00a1e839a57525c0ef782", "9f11919f67c65fdf34e5ee72b44b2192d9c303df207a788a8bffbca842fae69f"),
        ("d093546e6a17", "49bc68ef50ae5851f18edb0711353b51d51aeab102fca4de9023a31b6d08f3b8", "049ca563e954ca4bd76b3c519f7e04c9a3d00f7defb5e9b4a623b0217024b42d"),
        ("1e162ab3e346", "cb45114dfea693dda2ffbb0a23d732ed6aa5876ca7e2c563ab4648ec31f467e1", "9a02ecfe812e9688638b7f6d17a0ac61a47411ca4516c3c06d27bfc22a9b7f62"),
        ("46977ef37f91", "7650bfaf0d7a0cbca45cda4bd01efe2d616457259780d1c919a65b07c313e5db", "a9ba9cb5facf2b4d034d0fe020c77c77c2bdd554efcf730b47370629a219d75d"),
        ("a3ddbe0a4e1c", "b23f3266b62cf3b85e021fc1f478c83b76d55b52e000e28b0da7f6ff23bfa6e3", "05ec178edb9947c4bfa22c8ad7602ca33f0674c53667463f627213c1c4b991b6"),
    ];
    for (id, drawn, read) in expected {
        let s = sim_cad::stamp::stamps(&doc, id).unwrap();
        assert_eq!((s[0].as_str(), s[1].as_str()), (drawn, read), "node {id}");
    }
}

#[test]
fn hidden_line_views_keep_up_on_plus_y() {
    let b = kernel::build(&Shape::Box { corner: [0.; 3], size: [10., 20., 30.] }, &|| false).unwrap();
    // Front: looking along +y with z up, x right (x = d × up).
    let out = kernel::op(Op::Hlr, &[&b], &[0., 1., 0., 1., 0., 0.], &[], &|| false).unwrap();
    let t = kernel::full_topology(&out[0].brep, 24).unwrap();
    let ys: Vec<f64> = t["edges"].as_array().unwrap().iter().flat_map(|e| e["points"].as_array().unwrap().iter().map(|p| p[1].as_f64().unwrap())).collect();
    assert!(ys.iter().cloned().fold(f64::MIN, f64::max) > 29.9 && ys.iter().cloned().fold(f64::MAX, f64::min) > -0.1, "z 0..30 maps to y 0..30: {ys:?}");
}
