//! Local, work-preserving coordinate reduction: v = J u and a = J udot + b.
//! J and b must be recomputed when configuration or velocity changes. This is
//! not permission to freeze a nonlinear linkage throughout an episode.
use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorRegistry,
    primitive::{Descriptor, Field},
};

#[derive(Clone, Debug)]
pub struct LocalReduction {
    tangent: DMatrix<f64>,
    bias: DVector<f64>,
}
impl LocalReduction {
    pub fn new(tangent: DMatrix<f64>, acceleration_bias: DVector<f64>) -> Result<Self, String> {
        let gram = tangent.transpose() * &tangent;
        if tangent.ncols() == 0
            || tangent.nrows() < tangent.ncols()
            || acceleration_bias.len() != tangent.nrows()
            || tangent
                .iter()
                .chain(acceleration_bias.iter())
                .any(|v| !v.is_finite())
            || gram.iter().any(|v| !v.is_finite())
            || gram.cholesky().is_none()
        {
            return Err(
                "finite full-column-rank tangent and matching acceleration bias required".into(),
            );
        }
        Ok(Self {
            tangent,
            bias: acceleration_bias,
        })
    }
    fn vector(values: &[f64], n: usize) -> Result<DVector<f64>, String> {
        if values.len() != n || values.iter().any(|v| !v.is_finite()) {
            return Err("finite dimension-matched vector required".into());
        }
        Ok(DVector::from_column_slice(values))
    }
    fn checked(v: DVector<f64>) -> Result<Vec<f64>, String> {
        if v.iter().any(|x| !x.is_finite()) {
            return Err("reduction overflow".into());
        }
        Ok(v.as_slice().to_vec())
    }
    pub fn velocity(&self, reduced: &[f64]) -> Result<Vec<f64>, String> {
        Self::checked(&self.tangent * Self::vector(reduced, self.tangent.ncols())?)
    }
    pub fn acceleration(&self, reduced: &[f64]) -> Result<Vec<f64>, String> {
        Self::checked(&self.tangent * Self::vector(reduced, self.tangent.ncols())? + &self.bias)
    }
    /// Dual projection preserves instantaneous virtual work: f.v = (J^T f).u.
    pub fn force(&self, full: &[f64]) -> Result<Vec<f64>, String> {
        Self::checked(self.tangent.transpose() * Self::vector(full, self.tangent.nrows())?)
    }
    /// Reflected inertia preserves kinetic energy, including off-diagonal terms.
    pub fn inertia(&self, full: &DMatrix<f64>) -> Result<DMatrix<f64>, String> {
        if full.shape() != (self.tangent.nrows(), self.tangent.nrows())
            || full.iter().any(|v| !v.is_finite())
            || (full - full.transpose()).amax() > 1e-12 * (1. + full.amax())
            || full.clone().cholesky().is_none()
        {
            return Err("finite symmetric positive definite full inertia required".into());
        }
        let result = self.tangent.transpose() * full * &self.tangent;
        if result.iter().any(|v| !v.is_finite()) || result.clone().cholesky().is_none() {
            return Err("invalid reduced inertia".into());
        }
        Ok(result)
    }
}
impl crate::articulated::embedding::EmbeddedMotion {
    /// Export the exact CAD constraint tangent at this solved pose/velocity.
    pub fn local_reduction(&self) -> Result<LocalReduction, String> {
        LocalReduction::new(self.tangent.clone(), self.acceleration_bias.clone())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionRequest {
    pub tangent: Vec<Vec<f64>>,
    pub acceleration_bias: Vec<f64>,
    pub reduced_velocity: Vec<f64>,
    pub reduced_acceleration: Vec<f64>,
    pub full_force: Vec<f64>,
    pub full_inertia: Vec<Vec<f64>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub full_velocity: Vec<f64>,
    pub full_acceleration: Vec<f64>,
    pub reduced_force: Vec<f64>,
    pub reduced_inertia: Vec<Vec<f64>>,
}
fn matrix(rows: &[Vec<f64>]) -> Result<DMatrix<f64>, String> {
    let n = rows.first().map_or(0, Vec::len);
    if n == 0 || rows.iter().any(|r| r.len() != n) {
        return Err("nonempty rectangular matrix required".into());
    }
    Ok(DMatrix::from_fn(rows.len(), n, |i, j| rows[i][j]))
}
pub fn project(input: ProjectionRequest) -> Result<Projection, String> {
    let map = LocalReduction::new(
        matrix(&input.tangent)?,
        DVector::from_vec(input.acceleration_bias),
    )?;
    let mass = map.inertia(&matrix(&input.full_inertia)?)?;
    Ok(Projection {
        full_velocity: map.velocity(&input.reduced_velocity)?,
        full_acceleration: map.acceleration(&input.reduced_acceleration)?,
        reduced_force: map.force(&input.full_force)?,
        reduced_inertia: (0..mass.nrows())
            .map(|i| (0..mass.ncols()).map(|j| mass[(i, j)]).collect())
            .collect(),
    })
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), String> {
    registry.register_primitive(Descriptor::new("mechanics.local_reduction", "Work and inertia preserving local mechanism projection",
        vec![Field::structured("tangent", "full coordinate / reduced coordinate", "matrix"),
            Field::structured("acceleration_bias", "full coordinate/s²", "vector"),
            Field::structured("reduced_velocity", "reduced coordinate/s", "vector"),
            Field::structured("reduced_acceleration", "reduced coordinate/s²", "vector"),
            Field::structured("full_force", "J/full coordinate", "vector"),
            Field::structured("full_inertia", "J s²/(full coordinate_i full coordinate_j)", "matrix")],
        vec![Field::structured("full_velocity", "full coordinate/s", "vector"),
            Field::structured("full_acceleration", "full coordinate/s²", "vector"),
            Field::structured("reduced_force", "J/reduced coordinate", "vector"),
            Field::structured("reduced_inertia", "J s²/(reduced coordinate_i reduced coordinate_j)", "matrix")],
        &["Coordinates and frames are supplied by the CAD mechanism; recompute at each changed pose/velocity", "No discarded compliance, friction or inertia is inferred"]), project)
}
