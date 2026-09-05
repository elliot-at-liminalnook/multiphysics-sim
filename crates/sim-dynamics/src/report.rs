//! Named, numeric pass/fail expectations for a scenario run.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub observed: f64,
    pub expectation: String,
}

/// A recorded signal kept for plotting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    pub label: String,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub scenario: String,
    pub checks: Vec<Check>,
    /// Measured values worth showing even when nothing is asserted on them.
    pub measurements: Vec<(String, f64)>,
    pub series: Vec<Series>,
}

impl Report {
    pub fn new(scenario: impl Into<String>) -> Self {
        Self {
            scenario: scenario.into(),
            checks: Vec::new(),
            measurements: Vec::new(),
            series: Vec::new(),
        }
    }

    pub fn passed(&self) -> bool {
        self.checks.iter().all(|check| check.passed)
    }

    pub fn failures(&self) -> Vec<String> {
        self.checks
            .iter()
            .filter(|check| !check.passed)
            .map(|check| format!("{}: {:.6} ({})", check.name, check.observed, check.expectation))
            .collect()
    }

    /// Keep a signal for plotting, thinned to at most `limit` points.
    pub fn series(&mut self, label: &str, x: &[f64], y: &[f64], limit: usize) -> &mut Self {
        let stride = (x.len() / limit.max(1)).max(1);
        self.series.push(Series {
            label: label.to_owned(),
            x: x.iter().step_by(stride).copied().collect(),
            y: y.iter().step_by(stride).copied().collect(),
        });
        self
    }

    pub fn measure(&mut self, name: &str, value: f64) -> &mut Self {
        self.measurements.push((name.to_owned(), value));
        self
    }

    pub fn close(&mut self, name: &str, observed: f64, expected: f64, tolerance: f64) -> &mut Self {
        self.push(
            name,
            (observed - expected).abs() <= tolerance,
            observed,
            format!("within {tolerance} of {expected}"),
        )
    }

    /// Relative tolerance: `|observed − expected| ≤ fraction · |expected|`.
    pub fn within(&mut self, name: &str, observed: f64, expected: f64, fraction: f64) -> &mut Self {
        self.push(
            name,
            (observed - expected).abs() <= fraction * expected.abs(),
            observed,
            format!("within {:.1}% of {expected}", fraction * 100.0),
        )
    }

    pub fn below(&mut self, name: &str, observed: f64, maximum: f64) -> &mut Self {
        self.push(name, observed <= maximum, observed, format!("≤ {maximum}"))
    }

    pub fn above(&mut self, name: &str, observed: f64, minimum: f64) -> &mut Self {
        self.push(name, observed >= minimum, observed, format!("≥ {minimum}"))
    }

    pub fn holds(&mut self, name: &str, passed: bool) -> &mut Self {
        self.push(name, passed, f64::from(u8::from(passed)), "true".to_owned())
    }

    fn push(&mut self, name: &str, passed: bool, observed: f64, expectation: String) -> &mut Self {
        self.checks.push(Check {
            name: name.to_owned(),
            passed,
            observed,
            expectation,
        });
        self
    }
}
