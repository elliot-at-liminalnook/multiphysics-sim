//! Small, host-independent vector icons. Coordinates are in a 24 x 24 box.
//! UI hosts draw the same strokes; REST exposes the stable name and geometry.
pub const NAMES: &[&str] = &[
    "component",
    "motor",
    "gear",
    "spring",
    "damper",
    "battery",
    "resistor",
    "capacitor",
    "ground",
    "sensor",
    "controller",
    "thermal",
    "wheel",
    "screw",
    "subsystem",
];
pub fn for_type(id: &str) -> &'static str {
    let s = id.to_ascii_lowercase();
    for (words, icon) in [
        (
            &[
                "gearmotor",
                "motor",
                "servo",
                "stepper",
                "solenoid",
                "voice_coil",
            ][..],
            "motor",
        ),
        (&["gear", "belt", "pulley"][..], "gear"),
        (&["spring", "compliance"][..], "spring"),
        (&["damper", "friction", "brake"][..], "damper"),
        (&["battery", "source", "supply", "lipo"][..], "battery"),
        (&["resistor"][..], "resistor"),
        (&["capacitor", "capacitance"][..], "capacitor"),
        (&["ground"][..], "ground"),
        (
            &[
                "sensor",
                "sense",
                "encoder",
                "probe",
                "switch",
                "tachometer",
                "imu",
            ][..],
            "sensor",
        ),
        (
            &["control", "pid", "setpoint", "firmware", "pwm"][..],
            "controller",
        ),
        (&["thermal", "heat", "ambient"][..], "thermal"),
        (&["wheel", "propeller", "inertia"][..], "wheel"),
        (&["screw", "rack", "linear"][..], "screw"),
        (&["system", "assembly"][..], "subsystem"),
    ] {
        if words.iter().any(|w| s.contains(w)) {
            return icon;
        }
    }
    "component"
}
pub fn resolve<'a>(explicit: &'a str, id: &str) -> &'a str {
    if NAMES.contains(&explicit) {
        explicit
    } else {
        for_type(id)
    }
}
pub fn strokes(icon: &str) -> Vec<Vec<[f32; 2]>> {
    let path = |p: &[[f32; 2]]| p.to_vec();
    let circle = |x: f32, y: f32, r: f32| {
        (0..=24)
            .map(|i| {
                let a = i as f32 * std::f32::consts::TAU / 24.;
                [x + r * a.cos(), y + r * a.sin()]
            })
            .collect::<Vec<_>>()
    };
    match icon {
        "motor" => vec![
            path(&[[3., 6.], [17., 6.], [17., 18.], [3., 18.], [3., 6.]]),
            path(&[[17., 12.], [23., 12.]]),
            path(&[[6., 15.], [6., 9.], [10., 13.], [14., 9.], [14., 15.]]),
        ],
        "gear" => vec![
            (0..=32)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 32.;
                    let r = if i % 4 < 2 { 10. } else { 7. };
                    [12. + r * a.cos(), 12. + r * a.sin()]
                })
                .collect(),
            circle(12., 12., 3.),
        ],
        "spring" | "resistor" => vec![path(&[
            [1., 12.],
            [4., 12.],
            [6., 6.],
            [9., 18.],
            [12., 6.],
            [15., 18.],
            [18., 6.],
            [20., 12.],
            [23., 12.],
        ])],
        "damper" => vec![
            path(&[[1., 12.], [8., 12.], [8., 6.], [18., 6.]]),
            path(&[[8., 12.], [8., 18.], [18., 18.]]),
            path(&[[15., 8.], [15., 16.]]),
            path(&[[15., 12.], [23., 12.]]),
        ],
        "battery" => vec![
            path(&[[1., 12.], [8., 12.]]),
            path(&[[8., 4.], [8., 20.]]),
            path(&[[14., 8.], [14., 16.]]),
            path(&[[14., 12.], [23., 12.]]),
            path(&[[2., 5.], [6., 5.]]),
            path(&[[4., 3.], [4., 7.]]),
        ],
        "capacitor" => vec![
            path(&[[1., 12.], [9., 12.]]),
            path(&[[9., 4.], [9., 20.]]),
            path(&[[15., 4.], [15., 20.]]),
            path(&[[15., 12.], [23., 12.]]),
        ],
        "ground" => vec![
            path(&[[12., 2.], [12., 11.]]),
            path(&[[3., 11.], [21., 11.]]),
            path(&[[6., 16.], [18., 16.]]),
            path(&[[9., 21.], [15., 21.]]),
        ],
        "sensor" => vec![
            circle(12., 12., 9.),
            circle(12., 12., 2.),
            path(&[[12., 12.], [18., 6.]]),
        ],
        "controller" => vec![
            path(&[[5., 5.], [19., 5.], [19., 19.], [5., 19.], [5., 5.]]),
            path(&[[1., 9.], [5., 9.]]),
            path(&[[1., 15.], [5., 15.]]),
            path(&[[19., 12.], [23., 12.]]),
            path(&[[8., 15.], [10., 9.], [13., 15.], [16., 9.]]),
        ],
        "thermal" => vec![
            circle(12., 18., 4.),
            path(&[[9., 15.], [9., 4.], [12., 2.], [15., 4.], [15., 15.]]),
            path(&[[12., 7.], [12., 18.]]),
        ],
        "wheel" => vec![
            circle(12., 12., 10.),
            circle(12., 12., 3.),
            path(&[[2., 12.], [22., 12.]]),
            path(&[[12., 2.], [12., 22.]]),
        ],
        "screw" => vec![
            path(&[[2., 10.], [22., 10.]]),
            path(&[[2., 14.], [22., 14.]]),
            path(&[[7., 6.], [7., 18.], [17., 18.], [17., 6.], [7., 6.]]),
            path(&[[9., 14.], [13., 10.], [13., 14.], [17., 10.]]),
        ],
        "subsystem" => vec![
            path(&[[2., 2.], [22., 2.], [22., 22.], [2., 22.], [2., 2.]]),
            path(&[[6., 7.], [10., 7.], [10., 11.], [6., 11.], [6., 7.]]),
            path(&[[14., 13.], [18., 13.], [18., 17.], [14., 17.], [14., 13.]]),
            path(&[[10., 9.], [16., 9.], [16., 13.]]),
        ],
        _ => vec![
            path(&[
                [3., 7.],
                [12., 2.],
                [21., 7.],
                [21., 17.],
                [12., 22.],
                [3., 17.],
                [3., 7.],
                [12., 12.],
                [21., 7.],
            ]),
            path(&[[12., 12.], [12., 22.]]),
        ],
    }
}
/// Antialiased RGBA raster of the shared vectors for texture-based hosts.
pub fn rgba(icon: &str, size: usize) -> Vec<u8> {
    let lines = strokes(icon);
    let mut pixels = vec![0; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let p = [
                (x as f32 + 0.5) * 24. / size as f32,
                (y as f32 + 0.5) * 24. / size as f32,
            ];
            let mut distance = f32::INFINITY;
            for line in &lines {
                for w in line.windows(2) {
                    let v = [w[1][0] - w[0][0], w[1][1] - w[0][1]];
                    let q = [p[0] - w[0][0], p[1] - w[0][1]];
                    let t = ((q[0] * v[0] + q[1] * v[1]) / (v[0] * v[0] + v[1] * v[1]).max(1e-10))
                        .clamp(0., 1.);
                    distance = distance.min((q[0] - v[0] * t).hypot(q[1] - v[1] * t));
                }
            }
            let a = ((1.15 - distance) * size as f32 / 24.).clamp(0., 1.);
            pixels[(y * size + x) * 4..(y * size + x) * 4 + 4].copy_from_slice(&[
                235,
                242,
                250,
                (a * 255.) as u8,
            ]);
        }
    }
    pixels
}
