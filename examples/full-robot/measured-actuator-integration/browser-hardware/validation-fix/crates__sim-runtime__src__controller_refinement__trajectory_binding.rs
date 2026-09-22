//! Explicit coordinate-to-device binding for finite recorded-controller playback.
//! Resampling references is not a new plant simulation or an inferred calibration.
use super::fpga::Plan;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceTrace {
    pub source: String,
    pub source_sha256: String,
    pub coordinates: Vec<String>,
    pub times_s: Vec<f64>,
    pub targets_rad: Vec<Vec<f64>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub coordinate: String,
    pub motor_id: u8,
    pub polarity: i8,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Playback {
    pub bindings: Vec<Binding>,
    pub amplitude: f64,
    pub speed: f64,
    pub start_s: f64,
}

impl ReferenceTrace {
    pub fn validate(&self) -> Result<(), String> {
        if self.source.is_empty()
            || self.source_sha256.len() != 64
            || !self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.coordinates.is_empty()
            || self.times_s.len() < 2
            || self.times_s.len() != self.targets_rad.len()
            || self
                .coordinates
                .iter()
                .enumerate()
                .any(|(i, c)| c.is_empty() || self.coordinates[..i].contains(c))
            || self.times_s.iter().any(|t| !t.is_finite() || *t < 0.)
            || self.times_s.windows(2).any(|w| w[1] <= w[0])
            || self
                .targets_rad
                .iter()
                .any(|r| r.len() != self.coordinates.len() || r.iter().any(|v| !v.is_finite()))
        {
            return Err("Invalid named reference trace".into());
        }
        Ok(())
    }
    fn sample(&self, t: f64, c: usize) -> f64 {
        let b = self
            .times_s
            .partition_point(|v| *v < t)
            .min(self.times_s.len() - 1);
        if b == 0 {
            return self.targets_rad[0][c];
        }
        let a = b - 1;
        let f = (t - self.times_s[a]) / (self.times_s[b] - self.times_s[a]);
        self.targets_rad[a][c] + f * (self.targets_rad[b][c] - self.targets_rad[a][c])
    }
    /// A two-second, 100 Hz bench clip with the final 200 ms returning to home.
    /// Home is measured by the acquisition path immediately before each run.
    pub fn bind_bench_clip(
        &self,
        request: &Playback,
        template: &Plan,
        scope: &[u8],
    ) -> Result<Plan, String> {
        self.validate()?;
        if request.bindings.len() != 3
            || !request.amplitude.is_finite()
            || !(0.01..=0.25).contains(&request.amplitude)
            || !request.speed.is_finite()
            || !(0.25..=2.).contains(&request.speed)
            || !request.start_s.is_finite()
            || request.start_s < self.times_s[0]
            || request.start_s + 1.99 * request.speed > *self.times_s.last().unwrap()
        {
            return Err("Choose three distinct axes, amplitude 1–25%, speed 0.25–2x, and an available source interval".into());
        }
        let mut ids = Vec::new();
        let mut columns = Vec::new();
        for b in &request.bindings {
            let c = self
                .coordinates
                .iter()
                .position(|c| c == &b.coordinate)
                .ok_or("Unknown coordinate")?;
            if ids.contains(&b.motor_id) || columns.contains(&c) || ![-1, 1].contains(&b.polarity) {
                return Err(
                    "Bindings require unique motors, unique coordinates and explicit +/- polarity"
                        .into(),
                );
            }
            ids.push(b.motor_id);
            columns.push(c);
        }
        ids.sort_unstable();
        super::fpga::validate_physical_scope(scope, &ids)?;
        let mut plan = template.clone();
        plan.ids = ids;
        plan.period_s = 0.01;
        plan.name = format!("Bound recorded gait: {}", self.source);
        plan.role = "timing".into();
        plan.control = "fpga_device_pd".into();
        // Initial UI bring-up ceiling, independent of the firmware's full authority.
        plan.gains.limit = 100;
        plan.targets = (0..200)
            .map(|tick| {
                let t = tick as f64 * 0.01;
                let envelope = ((1.99 - t) / 0.20).clamp(0., 1.);
                let mut row = [0i16; 9];
                for (b, c) in request.bindings.iter().zip(&columns) {
                    let delta = (self.sample(request.start_s + t * request.speed, *c)
                        - self.sample(request.start_s, *c))
                        * request.amplitude
                        * f64::from(b.polarity)
                        * envelope
                        * 4096.
                        / std::f64::consts::TAU;
                    // Reject, never silently clip a gait or wrap an encoder command.
                    if !delta.is_finite() || delta.abs() > 80. {
                        return Err(format!("{} (motor {}): requested excursion {:.2}° at {:.2} s exceeds the {:.2}° bench limit (80 encoder counts). Reduce Motion scale.",
                            b.coordinate.trim_start_matches("joint."),b.motor_id,delta.abs()*360./4096.,t,80.*360./4096.));
                    }
                    row[usize::from(b.motor_id - 4)] = delta.round() as i16;
                }
                Ok(row)
            })
            .collect::<Result<Vec<_>, String>>()?;
        for (tick, rows) in plan.targets.windows(2).enumerate() {
            for b in &request.bindings {
                let axis = usize::from(b.motor_id - 4);
                let jump = (i32::from(rows[1][axis]) - i32::from(rows[0][axis])).unsigned_abs();
                if jump > 32 {
                    return Err(format!(
                        "{} (motor {}): target changes by {:.2}° in 10 ms at {:.2} s ({} encoder counts; limit 32 / {:.2}°). Reduce Motion scale or Gait speed.",
                        b.coordinate.trim_start_matches("joint."),
                        b.motor_id,
                        f64::from(jump) * 360. / 4096.,
                        (tick + 1) as f64 * 0.01,
                        jump,
                        32. * 360. / 4096.
                    ));
                }
            }
        }
        plan.validate()?;
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (ReferenceTrace, Playback, Plan) {
        let trace = ReferenceTrace {
            source: "fixture".into(),
            source_sha256: "a".repeat(64),
            coordinates: vec!["a".into(), "b".into(), "c".into()],
            times_s: vec![0., 4.],
            targets_rad: vec![vec![0.; 3], vec![1., 1.5, 2.]],
        };
        let request = Playback {
            bindings: (0..3)
                .map(|i| Binding {
                    coordinate: trace.coordinates[i].clone(),
                    motor_id: 10 + i as u8,
                    polarity: 1,
                })
                .collect(),
            amplitude: 0.1,
            speed: 1.,
            start_s: 0.,
        };
        let plan = Plan {
            control: "fpga_device_pd".into(),
            name: "fixture".into(),
            role: "timing".into(),
            ids: vec![10, 11, 12],
            period_s: 0.01,
            gains: sim_domain_control::fixed_pd::Gains {
                kp_q8: 4096,
                kd_q8: 0,
                kv_q8: 4096,
                limit: 100,
            },
            targets: vec![[0; 9]; 2],
            rms_limit_counts: 3.,
            peak_limit_counts: 10.,
            bitstream_path: "fixture.fs".into(),
            bitstream_blake3: "a".repeat(64),
        };
        (trace, request, plan)
    }
    #[test]
    fn binding_resamples_in_units_and_returns_home() {
        let (t, r, p) = fixture();
        let p = t.bind_bench_clip(&r, &p, &[10, 11, 12]).unwrap();
        assert_eq!(p.targets.len(), 200);
        assert_eq!(p.targets[0], [0; 9]);
        assert_eq!(p.targets[199], [0; 9]);
        assert_eq!(
            p.targets[100][6],
            (0.025 * 4096. / std::f64::consts::TAU).round() as i16
        );
        assert_eq!(&p.targets[100][..6], &[0; 6]);
    }
    #[test]
    fn rejects_aliases_unknown_scope_and_nonfinite_values() {
        let (t, r, p) = fixture();
        let mut bad = r.clone();
        bad.bindings[1].motor_id = 10;
        assert!(t.bind_bench_clip(&bad, &p, &[10, 11, 12]).is_err());
        bad = r.clone();
        bad.bindings[1].coordinate = "a".into();
        assert!(t.bind_bench_clip(&bad, &p, &[10, 11, 12]).is_err());
        assert!(t.bind_bench_clip(&r, &p, &[10, 11]).is_err());
        bad = r.clone();
        bad.speed = f64::NAN;
        assert!(t.bind_bench_clip(&bad, &p, &[10, 11, 12]).is_err());
        bad = r;
        bad.start_s = 4.;
        assert!(t.bind_bench_clip(&bad, &p, &[10, 11, 12]).is_err());
    }
    #[test]
    fn polarity_is_explicit_and_excess_travel_rejected() {
        let (mut t, mut r, p) = fixture();
        r.bindings[0].polarity = -1;
        assert!(t.bind_bench_clip(&r, &p, &[10, 11, 12]).unwrap().targets[100][6] < 0);
        t.targets_rad[1][0] = 100.;
        assert!(t.bind_bench_clip(&r, &p, &[10, 11, 12]).is_err());
    }
    #[test]
    fn real_gait_rejects_ten_percent_jump_and_accepts_nine_percent() {
        let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/full-robot/measured-actuator-integration/browser-hardware/reference-trace.json");
        let trace: ReferenceTrace = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let (_, mut r, p) = fixture();
        for (b, c) in r.bindings.iter_mut().zip(&trace.coordinates) {
            b.coordinate = c.clone();
        }
        let error = trace.bind_bench_clip(&r, &p, &[10, 11, 12]).unwrap_err();
        assert!(error.contains("Worm servo output (motor 11)"), "{error}");
        assert!(error.contains("33 encoder counts; limit 32"), "{error}");
        r.amplitude = 0.09;
        trace.bind_bench_clip(&r, &p, &[10, 11, 12]).unwrap();
        r.amplitude = 0.03;
        for leg in trace.coordinates.chunks_exact(3) {
            for (b, c) in r.bindings.iter_mut().zip(leg) {
                b.coordinate = c.clone();
            }
            trace.bind_bench_clip(&r, &p, &[10, 11, 12]).unwrap();
        }
    }
}
