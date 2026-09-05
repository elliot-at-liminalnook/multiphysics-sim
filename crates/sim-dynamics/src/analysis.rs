//! Signal analysis shared by every scenario: crossings, peaks, periods,
//! decay rates and Lyapunov exponents, all from a recorded trace.

/// Times at which `y` crosses `level` upward, linearly interpolated.
pub fn upward_crossings(t: &[f64], y: &[f64], level: f64) -> Vec<f64> {
    t.windows(2)
        .zip(y.windows(2))
        .filter(|(_, y)| y[0] < level && y[1] >= level)
        .map(|(t, y)| t[0] + (level - y[0]) / (y[1] - y[0]) * (t[1] - t[0]))
        .collect()
}

/// Local maxima as `(time, value)`, using a parabolic fit through neighbours.
pub fn peaks(t: &[f64], y: &[f64]) -> Vec<(f64, f64)> {
    (1..y.len().saturating_sub(1))
        .filter(|&i| y[i] > y[i - 1] && y[i] >= y[i + 1])
        .map(|i| {
            let (a, b, c) = (y[i - 1], y[i], y[i + 1]);
            let denominator = a - 2.0 * b + c;
            if denominator.abs() < 1.0e-300 {
                return (t[i], b);
            }
            let offset = 0.5 * (a - c) / denominator;
            let dt = t[i + 1] - t[i];
            (t[i] + offset * dt, b - 0.25 * (a - c) * offset)
        })
        .collect()
}

/// Median interval between successive upward crossings of the signal's mean.
pub fn period(t: &[f64], y: &[f64]) -> Option<f64> {
    let crossings = upward_crossings(t, y, mean(y));
    let mut intervals = crossings.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>();
    intervals.sort_by(|a, b| a.total_cmp(b));
    (!intervals.is_empty()).then(|| intervals[intervals.len() / 2])
}

pub fn mean(y: &[f64]) -> f64 {
    y.iter().sum::<f64>() / y.len().max(1) as f64
}

pub fn rms(y: &[f64]) -> f64 {
    (y.iter().map(|v| v * v).sum::<f64>() / y.len().max(1) as f64).sqrt()
}

pub fn max_abs(y: &[f64]) -> f64 {
    y.iter().fold(0.0_f64, |m, v| m.max(v.abs()))
}

pub fn max(y: &[f64]) -> f64 {
    y.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

pub fn min(y: &[f64]) -> f64 {
    y.iter().copied().fold(f64::INFINITY, f64::min)
}

/// Exponential growth rate of the peak envelope, from a least-squares line
/// through `ln|peak|` against time. Negative means decay.
pub fn envelope_rate(t: &[f64], y: &[f64]) -> Option<f64> {
    let points = peaks(t, y)
        .into_iter()
        .filter(|(_, value)| *value > 0.0)
        .map(|(time, value)| (time, value.ln()))
        .collect::<Vec<_>>();
    linear_fit(&points).map(|(slope, _)| slope)
}

/// Slope and intercept of the least-squares line through `(x, y)` points.
pub fn linear_fit(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = points.len() as f64;
    if points.len() < 2 {
        return None;
    }
    let (sx, sy) = points.iter().fold((0.0, 0.0), |(a, b), (x, y)| (a + x, b + y));
    let (mx, my) = (sx / n, sy / n);
    let (sxx, sxy) = points.iter().fold((0.0, 0.0), |(a, b), (x, y)| {
        (a + (x - mx) * (x - mx), b + (x - mx) * (y - my))
    });
    (sxx > 0.0).then(|| (sxy / sxx, my - sxy / sxx * mx))
}

/// Largest Lyapunov exponent by the Benettin renormalisation method.
///
/// `advance(state, duration)` integrates a state in place; the reference and
/// a neighbour separated by `separation` are advanced `renormalisations`
/// times and the mean log stretch per unit time is returned.
pub fn largest_lyapunov_exponent(
    mut reference: Vec<f64>,
    separation: f64,
    interval: f64,
    renormalisations: usize,
    mut advance: impl FnMut(&mut [f64], f64),
) -> f64 {
    let n = reference.len();
    let mut neighbour = reference.clone();
    neighbour[0] += separation;
    let mut sum = 0.0;
    for _ in 0..renormalisations {
        advance(&mut reference, interval);
        advance(&mut neighbour, interval);
        let distance = (0..n)
            .map(|i| (neighbour[i] - reference[i]).powi(2))
            .sum::<f64>()
            .sqrt();
        sum += (distance / separation).ln();
        for i in 0..n {
            neighbour[i] = reference[i] + (neighbour[i] - reference[i]) * separation / distance;
        }
    }
    sum / (renormalisations as f64 * interval)
}

/// Number of distinct values in a sequence of Poincaré-section samples, with
/// values closer than `tolerance` merged. Period-1 → 1, period-2 → 2, and a
/// chaotic section never repeats.
pub fn distinct_values(samples: &[f64], tolerance: f64) -> usize {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    sorted.dedup_by(|a, b| (*a - *b).abs() <= tolerance);
    sorted.len()
}

/// Smallest `p` (up to `max_period`) for which the sequence repeats with
/// period `p` to within `tolerance` over its whole length. `None` means the
/// sequence is not periodic at this tolerance — chaotic, or still settling.
pub fn minimal_period(samples: &[f64], tolerance: f64, max_period: usize) -> Option<usize> {
    (1..=max_period).find(|&p| {
        samples.len() > 2 * p
            && samples
                .iter()
                .zip(samples.iter().skip(p))
                .all(|(a, b)| (a - b).abs() <= tolerance)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_and_envelope_of_damped_sine() {
        let t = (0..20_000).map(|i| i as f64 * 1.0e-3).collect::<Vec<_>>();
        let y = t
            .iter()
            .map(|t| (-0.3 * t).exp() * (2.0 * std::f64::consts::PI * 2.5 * t).sin())
            .collect::<Vec<_>>();
        assert!((period(&t, &y).unwrap() - 0.4).abs() < 1.0e-3);
        assert!((envelope_rate(&t, &y).unwrap() + 0.3).abs() < 1.0e-3);
    }

    #[test]
    fn minimal_period_finds_repeats() {
        assert_eq!(minimal_period(&[1.0, 2.0, 1.0, 2.0, 1.0], 1.0e-9, 8), Some(2));
        assert_eq!(minimal_period(&[1.0, 2.0, 3.0, 4.0, 5.0], 1.0e-9, 8), None);
    }

    #[test]
    fn distinct_values_counts_orbit_period() {
        assert_eq!(distinct_values(&[1.0, 2.0, 1.0000001, 2.0], 1.0e-4), 2);
    }
}

/// Monodromy matrix of a flow over one period, by central differences:
/// column `j` is `(flow(x₀ + εeⱼ) − flow(x₀ − εeⱼ)) / 2ε` with
/// `ε = epsilon·(1 + |x₀ⱼ|)`. `flow` maps a state at the start of the
/// period to the state one period later. A spinning top, a walker's
/// stride, any orbit that repeats: its Floquet multipliers are this
/// matrix's eigenvalues, and the orbit is stable when they all sit inside
/// the unit circle (apart from the neutral ones).
///
/// `epsilon` must sit well above the flow's own convergence noise — an
/// implicit integrator returns states to ~1e-9, so 1e-4 is a sound
/// default — and the period must bring the state *itself* back: a
/// rotating body's quaternion returns as `−q` after one turn, so its
/// period is two turns.
pub fn monodromy(mut flow: impl FnMut(&[f64]) -> Vec<f64>, x0: &[f64], epsilon: f64) -> nalgebra::DMatrix<f64> {
    let n = x0.len();
    let mut m = nalgebra::DMatrix::zeros(n, n);
    for j in 0..n {
        let eps = epsilon * (1.0 + x0[j].abs());
        let mut plus = x0.to_vec();
        let mut minus = x0.to_vec();
        plus[j] += eps;
        minus[j] -= eps;
        let (fp, fm) = (flow(&plus), flow(&minus));
        for i in 0..n {
            m[(i, j)] = (fp[i] - fm[i]) / (2.0 * eps);
        }
    }
    m
}

/// Floquet multipliers of a periodic orbit: the eigenvalues of
/// [`monodromy`]. Pass the neutral directions of the orbit (its own
/// tangent, a conserved-quantity gradient) in `neutral`; they are projected
/// out first so their unit multipliers do not mask a marginal one.
pub fn floquet_multipliers(
    flow: impl FnMut(&[f64]) -> Vec<f64>,
    x0: &[f64],
    epsilon: f64,
    neutral: &[Vec<f64>],
) -> Vec<nalgebra::Complex<f64>> {
    let n = x0.len();
    let mut m = monodromy(flow, x0, epsilon);
    let mut projector = nalgebra::DMatrix::identity(n, n);
    for direction in neutral {
        let d = nalgebra::DVector::from_column_slice(direction);
        let norm2 = d.dot(&d);
        if norm2 > 0.0 {
            projector -= &d * d.transpose() / norm2;
        }
    }
    m = &projector * m * &projector;
    m.complex_eigenvalues().iter().copied().filter(|e| e.norm() > 1.0e-9).collect()
}

/// In-place radix-2 FFT of complex samples (length a power of two).
pub fn fft(data: &mut [nalgebra::Complex<f64>]) {
    let n = data.len();
    assert!(n.is_power_of_two(), "fft length must be a power of two");
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            data.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f64::consts::PI / len as f64;
        let w_len = nalgebra::Complex::new(angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let mut w = nalgebra::Complex::new(1.0, 0.0);
            for k in 0..len / 2 {
                let u = data[start + k];
                let v = data[start + k + len / 2] * w;
                data[start + k] = u + v;
                data[start + k + len / 2] = u - v;
                w *= w_len;
            }
        }
        len <<= 1;
    }
}

/// One-sided power spectral density of a uniformly sampled series (mean
/// removed, Hann window, zero-padded to a power of two): `(frequency, psd)`
/// pairs with `psd` in units²/Hz.
pub fn power_spectrum(t: &[f64], y: &[f64]) -> Vec<(f64, f64)> {
    let n = y.len().min(t.len());
    if n < 4 {
        return Vec::new();
    }
    let dt = (t[n - 1] - t[0]) / (n as f64 - 1.0);
    let m = mean(&y[..n]);
    let size = n.next_power_of_two();
    let mut data = vec![nalgebra::Complex::new(0.0, 0.0); size];
    let mut window_power = 0.0;
    for k in 0..n {
        let w = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * k as f64 / (n as f64 - 1.0)).cos());
        window_power += w * w;
        data[k] = nalgebra::Complex::new(w * (y[k] - m), 0.0);
    }
    fft(&mut data);
    let df = 1.0 / (size as f64 * dt);
    (1..size / 2)
        .map(|k| (k as f64 * df, 2.0 * data[k].norm_sqr() * dt / window_power))
        .collect()
}
