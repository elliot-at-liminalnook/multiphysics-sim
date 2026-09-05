//! The physical assembly description the CAD tool writes (`simrobot` v3):
//! links with full inertia, collision meshes and signed-distance grids,
//! modal flexibility, joints with the physics their geometry implies,
//! motors with electrical, gearbox, thermal and firmware blocks, sensors,
//! cables, a battery, control targets and uncertainty. Every field has a
//! default so a partial file still loads; SI units throughout.
//! See `cad/PHYSICAL_MODEL.md`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

pub type V3 = [f64; 3];

fn default_gravity() -> V3 {
    [0.0, 0.0, -9.81]
}
fn one() -> f64 {
    1.0
}
fn default_version() -> u32 {
    3
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhysicalModel {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub source: serde_json::Value,
    #[serde(default = "default_gravity")]
    pub gravity: V3,
    #[serde(default)]
    pub world: World,
    #[serde(default)]
    pub materials: BTreeMap<String, Material>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub joints: Vec<Joint>,
    #[serde(default)]
    pub motors: Vec<Motor>,
    #[serde(default)]
    pub transmissions: Vec<Transmission>,
    #[serde(default)]
    pub battery: Option<Battery>,
    #[serde(default)]
    pub sensors: Vec<Sensor>,
    #[serde(default)]
    pub cables: Vec<Cable>,
    #[serde(default)]
    pub control: Control,
    #[serde(default)]
    pub uncertainty: Uncertainty,
    #[serde(default)]
    pub identification: BTreeMap<String, Identification>,
    #[serde(default)]
    pub planar: Option<PlanarHint>,
}

/// Ideal signed angular constraint: driver angle = ratio * driven angle.
/// Coordinates are changes from the exported assembly pose. Losses require
/// separately identified friction/backlash; this constraint conserves power.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transmission {
    pub name: String,
    pub driver_joint: String,
    pub driven_joint: String,
    pub ratio: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct World {
    #[serde(default)]
    pub floor_z: f64,
    #[serde(default = "World::default_friction")]
    pub floor_friction: f64,
    #[serde(default = "World::default_stiffness")]
    pub floor_stiffness: f64,
    #[serde(default = "World::default_damping")]
    pub floor_damping: f64,
    #[serde(default)]
    pub terrain: Option<Terrain>,
    #[serde(default = "World::default_ambient")]
    pub ambient_c: f64,
}
impl World {
    fn default_friction() -> f64 {
        0.8
    }
    fn default_stiffness() -> f64 {
        2.0e5
    }
    fn default_damping() -> f64 {
        2.0e3
    }
    fn default_ambient() -> f64 {
        20.0
    }
}
impl Default for World {
    fn default() -> Self {
        Self { floor_z: 0.0, floor_friction: 0.8, floor_stiffness: 2.0e5, floor_damping: 2.0e3, terrain: None, ambient_c: 20.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Terrain {
    pub origin: [f64; 2],
    pub cell: f64,
    pub dims: [usize; 2],
    pub heights: Vec<f64>,
}
impl Terrain {
    /// Bilinear height at `(x, y)`, clamped to the grid.
    pub fn height(&self, x: f64, y: f64) -> f64 {
        let (nx, ny) = (self.dims[0], self.dims[1]);
        if nx < 2 || ny < 2 || self.heights.len() < nx * ny {
            return self.heights.first().copied().unwrap_or(0.0);
        }
        let fx = ((x - self.origin[0]) / self.cell).clamp(0.0, (nx - 1) as f64 - 1e-9);
        let fy = ((y - self.origin[1]) / self.cell).clamp(0.0, (ny - 1) as f64 - 1e-9);
        let (ix, iy) = (fx.floor() as usize, fy.floor() as usize);
        let (tx, ty) = (fx - ix as f64, fy - iy as f64);
        let h = |i: usize, j: usize| self.heights[i * ny + j];
        (1.0 - tx) * (1.0 - ty) * h(ix, iy) + tx * (1.0 - ty) * h(ix + 1, iy) + (1.0 - tx) * ty * h(ix, iy + 1) + tx * ty * h(ix + 1, iy + 1)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrictionPair {
    #[serde(default = "FrictionPair::default_static")]
    pub r#static: f64,
    #[serde(default = "FrictionPair::default_kinetic")]
    pub kinetic: f64,
}
impl FrictionPair {
    fn default_static() -> f64 {
        0.5
    }
    fn default_kinetic() -> f64 {
        0.4
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PrintProps {
    #[serde(default = "one")]
    pub anisotropy_z: f64,
    #[serde(default = "one")]
    pub layer_adhesion_factor: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Material {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "Material::default_density")]
    pub density: f64,
    #[serde(default = "Material::default_modulus")]
    pub youngs_modulus: f64,
    #[serde(default = "Material::default_poisson")]
    pub poisson: f64,
    #[serde(default = "Material::default_yield")]
    pub yield_strength: f64,
    #[serde(default = "Material::default_ultimate")]
    pub ultimate_strength: f64,
    #[serde(default = "Material::default_tg")]
    pub glass_transition_c: f64,
    #[serde(default = "Material::default_k")]
    pub thermal_conductivity: f64,
    #[serde(default = "Material::default_cp")]
    pub specific_heat: f64,
    #[serde(default)]
    pub thermal_expansion: f64,
    #[serde(default)]
    pub friction: BTreeMap<String, FrictionPair>,
    #[serde(default)]
    pub print: Option<PrintProps>,
}
impl Material {
    fn default_density() -> f64 {
        1240.0
    }
    fn default_modulus() -> f64 {
        3.0e9
    }
    fn default_poisson() -> f64 {
        0.35
    }
    fn default_yield() -> f64 {
        50.0e6
    }
    fn default_ultimate() -> f64 {
        60.0e6
    }
    fn default_tg() -> f64 {
        60.0
    }
    fn default_k() -> f64 {
        0.2
    }
    fn default_cp() -> f64 {
        1800.0
    }
}
impl Default for Material {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Sdf {
    #[serde(default)]
    pub origin: V3,
    #[serde(default = "one")]
    pub cell: f64,
    #[serde(default)]
    pub dims: [usize; 3],
    #[serde(default)]
    pub values: Vec<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Collision {
    #[serde(default)]
    pub vertices: Vec<V3>,
    #[serde(default)]
    pub triangles: Vec<[usize; 3]>,
    #[serde(default)]
    pub hull: Vec<V3>,
    #[serde(default)]
    pub sdf: Option<Sdf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Softening {
    #[serde(default = "Softening::default_tg")]
    pub tg_c: f64,
    #[serde(default = "Softening::default_width")]
    pub width_c: f64,
    #[serde(default = "Softening::default_ratio")]
    pub ratio_above: f64,
}
impl Softening {
    fn default_tg() -> f64 {
        60.0
    }
    fn default_width() -> f64 {
        10.0
    }
    fn default_ratio() -> f64 {
        0.05
    }
    /// Stiffness multiplier at temperature `t` (°C).
    pub fn factor(&self, t: f64) -> f64 {
        let s = 1.0 / (1.0 + (-(t - self.tg_c) / self.width_c.max(1e-6)).exp());
        self.ratio_above + (1.0 - self.ratio_above) * (1.0 - s)
    }
}
impl Default for Softening {
    fn default() -> Self {
        Self { tg_c: 60.0, width_c: 10.0, ratio_above: 0.05 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BoundaryFrame {
    /// Captured CAD attachment identity; names remain display labels.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub point: V3,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModalNormalization {
    /// Legacy/custom modal bases whose amplitude convention was not declared.
    #[default]
    Unspecified,
    /// Shape translations are dimensionless; amplitudes are metres.
    Displacement,
    /// Shapes satisfy Phi^T M Phi = I; amplitudes are metres sqrt(kg).
    MassNormalized,
}

impl ModalNormalization {
    pub fn quantities(self) -> (sim_core::QuantityKind, sim_core::QuantityKind) {
        use sim_core::QuantityKind::*;
        match self {
            Self::Unspecified => (ModalCoordinate, ModalVelocity),
            Self::Displacement => (Length, LinearVelocity),
            Self::MassNormalized => (MassNormalizedDisplacement, MassNormalizedVelocity),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Flex {
    #[serde(default)]
    pub normalization: ModalNormalization,
    #[serde(default)]
    pub modes: usize,
    #[serde(default)]
    pub frequencies_hz: Vec<f64>,
    #[serde(default = "Flex::default_damping")]
    pub damping_ratio: f64,
    #[serde(default)]
    pub boundary_frames: Vec<BoundaryFrame>,
    #[serde(default)]
    pub modal_stiffness: Vec<f64>,
    #[serde(default)]
    pub modal_mass: Vec<f64>,
    /// `[mode][boundary][6]`
    #[serde(default)]
    pub boundary_shapes: Vec<Vec<[f64; 6]>>,
    /// `[mode][6]`
    #[serde(default)]
    pub participation: Vec<[f64; 6]>,
    #[serde(default)]
    pub stress_cells: Vec<V3>,
    /// `[mode][cell][6]`
    #[serde(default)]
    pub stress_per_mode: Vec<Vec<[f64; 6]>>,
    #[serde(default)]
    pub gravity_sag_m: f64,
    #[serde(default)]
    pub softening: Softening,
}
impl Flex {
    fn default_damping() -> f64 {
        0.03
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PrintSetup {
    #[serde(default)]
    pub orientation: V3,
    #[serde(default)]
    pub infill: f64,
    #[serde(default)]
    pub walls: f64,
    #[serde(default)]
    pub layer_height: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Link {
    pub name: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub ground: bool,
    #[serde(default = "one")]
    pub mass: f64,
    #[serde(default)]
    pub com: V3,
    #[serde(default = "Link::default_inertia")]
    pub inertia: [[f64; 3]; 3],
    #[serde(default)]
    pub bbox: Vec<V3>,
    #[serde(default)]
    pub collision: Collision,
    #[serde(default)]
    pub flex: Option<Flex>,
    #[serde(default)]
    pub print: Option<PrintSetup>,
}
impl Link {
    fn default_inertia() -> [[f64; 3]; 3] {
        [[1e-6, 0.0, 0.0], [0.0, 1e-6, 0.0], [0.0, 0.0, 1e-6]]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Friction {
    #[serde(default)]
    pub coulomb: f64,
    #[serde(default)]
    pub viscous: f64,
    #[serde(default)]
    pub stribeck: f64,
    #[serde(default = "Friction::default_speed")]
    pub stribeck_speed: f64,
    #[serde(default = "Friction::default_static_ratio")]
    pub static_ratio: f64,
}
impl Friction {
    fn default_speed() -> f64 {
        0.05
    }
    fn default_static_ratio() -> f64 {
        1.2
    }
}
impl Default for Friction {
    fn default() -> Self {
        Self { coulomb: 0.0, viscous: 0.0, stribeck: 0.0, stribeck_speed: 0.05, static_ratio: 1.2 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JointStiffness {
    #[serde(default = "JointStiffness::default_linear")]
    pub radial: f64,
    #[serde(default = "JointStiffness::default_linear")]
    pub axial: f64,
    #[serde(default = "JointStiffness::default_bending")]
    pub bending: f64,
}
impl JointStiffness {
    fn default_linear() -> f64 {
        2.0e6
    }
    fn default_bending() -> f64 {
        50.0
    }
}
impl Default for JointStiffness {
    fn default() -> Self {
        Self { radial: 2.0e6, axial: 2.0e6, bending: 50.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Bearing {
    #[serde(default)]
    pub kind: String,
    #[serde(default = "Bearing::default_pressure")]
    pub allowable_pressure: f64,
}
impl Bearing {
    fn default_pressure() -> f64 {
        10.0e6
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JointPhysics {
    #[serde(default)]
    pub source: String,
    #[serde(default = "JointPhysics::default_radius")]
    pub pin_radius: f64,
    #[serde(default = "JointPhysics::default_radius")]
    pub hole_radius: f64,
    #[serde(default = "JointPhysics::default_length")]
    pub contact_length: f64,
    #[serde(default)]
    pub clearance: f64,
    #[serde(default)]
    pub backlash: f64,
    #[serde(default)]
    pub wobble: f64,
    #[serde(default)]
    pub friction: Friction,
    #[serde(default)]
    pub stiffness: JointStiffness,
    #[serde(default = "JointPhysics::default_damping_ratio")]
    pub damping_ratio: f64,
    #[serde(default)]
    pub bearing: Bearing,
}
impl JointPhysics {
    fn default_radius() -> f64 {
        2.0e-3
    }
    fn default_length() -> f64 {
        5.0e-3
    }
    fn default_damping_ratio() -> f64 {
        0.05
    }
}
impl Default for JointPhysics {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Fastened {
    #[serde(default)]
    pub screw: Option<String>,
    #[serde(default)]
    pub count: f64,
    #[serde(default)]
    pub preload: f64,
    #[serde(default)]
    pub stiffness: f64,
    #[serde(default)]
    pub shear_capacity: f64,
    #[serde(default)]
    pub pattern_radius: f64,
}

fn default_axis() -> V3 {
    [0.0, 0.0, 1.0]
}
fn default_joint_type() -> String {
    "revolute".to_owned()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Joint {
    pub name: String,
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_joint_type", rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub parent: Option<String>,
    pub child: String,
    #[serde(default)]
    pub origin: V3,
    #[serde(default = "default_axis")]
    pub axis: V3,
    #[serde(default)]
    pub limits: Option<[f64; 2]>,
    #[serde(default)]
    pub home: f64,
    #[serde(default)]
    pub physics: JointPhysics,
    #[serde(default)]
    pub fastened: Option<Fastened>,
    #[serde(default)]
    pub motor: Option<String>,
}
impl Joint {
    pub fn is_loop(&self) -> bool {
        self.kind.starts_with("loop_")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MotorElectrical {
    #[serde(default = "MotorElectrical::default_r")]
    pub resistance: f64,
    #[serde(default = "MotorElectrical::default_l")]
    pub inductance: f64,
    #[serde(default = "MotorElectrical::default_kt")]
    pub torque_constant: f64,
    #[serde(default = "MotorElectrical::default_kt")]
    pub back_emf_constant: f64,
    #[serde(default)]
    pub no_load_current: f64,
    #[serde(default = "MotorElectrical::default_rotor")]
    pub rotor_inertia: f64,
    #[serde(default = "MotorElectrical::default_v")]
    pub supply_voltage: f64,
    #[serde(default = "MotorElectrical::default_i")]
    pub current_limit: f64,
    #[serde(default)]
    pub poles: f64,
}
impl MotorElectrical {
    fn default_r() -> f64 {
        4.0
    }
    fn default_l() -> f64 {
        1.0e-3
    }
    fn default_kt() -> f64 {
        0.01
    }
    fn default_rotor() -> f64 {
        1.0e-7
    }
    fn default_v() -> f64 {
        6.0
    }
    fn default_i() -> f64 {
        2.0
    }
}
impl Default for MotorElectrical {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gearbox {
    #[serde(default = "one")]
    pub ratio: f64,
    #[serde(default = "Gearbox::default_eff")]
    pub efficiency: f64,
    #[serde(default)]
    pub backlash_rad: f64,
    #[serde(default)]
    pub inertia: f64,
    #[serde(default = "Gearbox::default_stiffness")]
    pub stiffness: f64,
    #[serde(default = "Gearbox::default_torque")]
    pub max_output_torque: f64,
    #[serde(default = "Gearbox::default_speed")]
    pub max_output_speed: f64,
}
impl Gearbox {
    fn default_eff() -> f64 {
        0.8
    }
    fn default_stiffness() -> f64 {
        200.0
    }
    fn default_torque() -> f64 {
        f64::INFINITY
    }
    fn default_speed() -> f64 {
        f64::INFINITY
    }
}
impl Default for Gearbox {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MotorThermal {
    #[serde(default = "MotorThermal::default_cw")]
    pub winding_heat_capacity: f64,
    #[serde(default = "MotorThermal::default_cc")]
    pub case_heat_capacity: f64,
    #[serde(default = "MotorThermal::default_rwc")]
    pub r_winding_case: f64,
    #[serde(default = "MotorThermal::default_rcm")]
    pub r_case_mount: f64,
    #[serde(default = "MotorThermal::default_rca")]
    pub r_case_ambient: f64,
    #[serde(default = "MotorThermal::default_alpha")]
    pub resistance_temp_coeff: f64,
    #[serde(default = "MotorThermal::default_derating")]
    pub torque_derating_per_c: f64,
    #[serde(default = "MotorThermal::default_max")]
    pub max_winding_c: f64,
}
impl MotorThermal {
    fn default_cw() -> f64 {
        5.0
    }
    fn default_cc() -> f64 {
        20.0
    }
    fn default_rwc() -> f64 {
        4.0
    }
    fn default_rcm() -> f64 {
        20.0
    }
    fn default_rca() -> f64 {
        30.0
    }
    fn default_alpha() -> f64 {
        0.0039
    }
    fn default_derating() -> f64 {
        0.001
    }
    fn default_max() -> f64 {
        120.0
    }
}
impl Default for MotorThermal {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Firmware {
    #[serde(default = "Firmware::default_kind")]
    pub kind: String,
    #[serde(default = "Firmware::default_rate")]
    pub loop_rate_hz: f64,
    #[serde(default)]
    pub latency_s: f64,
    #[serde(default)]
    pub deadband_rad: f64,
    #[serde(default)]
    pub sensor_resolution_rad: f64,
    #[serde(default = "Firmware::default_kp")]
    pub kp: f64,
    #[serde(default)]
    pub ki: f64,
    #[serde(default)]
    pub kd: f64,
    #[serde(default = "Firmware::default_output")]
    pub output: String,
}
impl Firmware {
    fn default_kind() -> String {
        "servo".to_owned()
    }
    fn default_rate() -> f64 {
        50.0
    }
    fn default_kp() -> f64 {
        20.0
    }
    fn default_output() -> String {
        "voltage".to_owned()
    }
}
impl Default for Firmware {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Driver {
    #[serde(default = "Driver::default_kind")]
    pub kind: String,
    #[serde(default = "Driver::default_pwm")]
    pub pwm_hz: f64,
    #[serde(default = "Driver::default_ron")]
    pub on_resistance: f64,
    #[serde(default = "Driver::default_limit")]
    pub current_limit: f64,
}
impl Driver {
    fn default_kind() -> String {
        "h_bridge".to_owned()
    }
    fn default_pwm() -> f64 {
        20_000.0
    }
    fn default_ron() -> f64 {
        0.1
    }
    fn default_limit() -> f64 {
        3.0
    }
}
impl Default for Driver {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Motor {
    pub name: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub spec: String,
    #[serde(default)]
    pub joint: Option<String>,
    #[serde(default)]
    pub mounted_on: Option<String>,
    #[serde(default)]
    pub mount_point: V3,
    #[serde(default = "default_axis")]
    pub shaft_axis: V3,
    #[serde(default = "one")]
    pub gear_ratio: f64,
    #[serde(default)]
    pub electrical: MotorElectrical,
    #[serde(default)]
    pub gearbox: Gearbox,
    #[serde(default)]
    pub thermal: MotorThermal,
    #[serde(default)]
    pub firmware: Firmware,
    #[serde(default)]
    pub driver: Driver,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Battery {
    #[serde(default = "Battery::default_cells")]
    pub cells: f64,
    #[serde(default = "Battery::default_v")]
    pub nominal_voltage: f64,
    #[serde(default = "Battery::default_r")]
    pub internal_resistance: f64,
    #[serde(default = "one")]
    pub capacity_ah: f64,
    #[serde(default = "one")]
    pub initial_soc: f64,
    #[serde(default = "Battery::default_cutoff")]
    pub cutoff_voltage: f64,
}
impl Battery {
    fn default_cells() -> f64 {
        2.0
    }
    fn default_v() -> f64 {
        7.4
    }
    fn default_r() -> f64 {
        0.05
    }
    fn default_cutoff() -> f64 {
        6.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SensorNoise {
    #[serde(default)]
    pub accel: f64,
    #[serde(default)]
    pub gyro: f64,
    #[serde(default)]
    pub angle: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SensorBias {
    #[serde(default)]
    pub accel: V3,
    #[serde(default)]
    pub gyro: V3,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Quantization {
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub accel: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SensorRange {
    #[serde(default)]
    pub accel: f64,
    #[serde(default)]
    pub gyro: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sensor {
    pub name: String,
    #[serde(default)]
    pub id: String,
    #[serde(default = "Sensor::default_kind")]
    pub kind: String,
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub point: V3,
    #[serde(default = "Sensor::default_axes")]
    pub axes: [[f64; 3]; 3],
    #[serde(default = "Sensor::default_rate")]
    pub rate_hz: f64,
    #[serde(default)]
    pub noise: SensorNoise,
    #[serde(default)]
    pub bias: SensorBias,
    #[serde(default)]
    pub bias_walk: f64,
    #[serde(default)]
    pub quantization: Quantization,
    #[serde(default)]
    pub joint: Option<String>,
    #[serde(default)]
    pub range: SensorRange,
}
impl Sensor {
    fn default_kind() -> String {
        "imu".to_owned()
    }
    fn default_axes() -> [[f64; 3]; 3] {
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    }
    fn default_rate() -> f64 {
        200.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Attachment {
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub point: V3,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cable {
    pub name: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub from: Attachment,
    #[serde(default)]
    pub to: Attachment,
    #[serde(default)]
    pub length: f64,
    #[serde(default)]
    pub mass: f64,
    #[serde(default = "Cable::default_stiffness")]
    pub stiffness: f64,
    #[serde(default)]
    pub damping: f64,
    #[serde(default = "Cable::default_segments")]
    pub segments: f64,
}
impl Cable {
    fn default_stiffness() -> f64 {
        500.0
    }
    fn default_segments() -> f64 {
        4.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrajectoryPoint {
    pub t: f64,
    #[serde(default)]
    pub targets: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Control {
    #[serde(default = "Control::default_period")]
    pub period_s: f64,
    #[serde(default)]
    pub latency_s: f64,
    #[serde(default)]
    pub targets: BTreeMap<String, f64>,
    #[serde(default = "Control::default_mode")]
    pub mode: String,
    #[serde(default)]
    pub trajectory: Vec<TrajectoryPoint>,
}
impl Control {
    fn default_period() -> f64 {
        0.005
    }
    fn default_mode() -> String {
        "hold".to_owned()
    }
}
impl Default for Control {
    fn default() -> Self {
        Self { period_s: 0.005, latency_s: 0.0, targets: BTreeMap::new(), mode: "hold".into(), trajectory: Vec::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Sigma {
    #[serde(default)]
    pub sigma: f64,
    #[serde(default)]
    pub sigma_fraction: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Uncertainty {
    #[serde(default)]
    pub dimension_m: Sigma,
    #[serde(default)]
    pub mass: Sigma,
    #[serde(default)]
    pub friction: Sigma,
    #[serde(default)]
    pub stiffness: Sigma,
    #[serde(default)]
    pub backlash: Sigma,
    #[serde(default)]
    pub motor_torque: Sigma,
    #[serde(default)]
    pub com_m: Sigma,
    #[serde(default)]
    pub seed: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Identification {
    #[serde(default)]
    pub friction: Option<Friction>,
    #[serde(default)]
    pub backlash: Option<f64>,
    #[serde(default)]
    pub stiffness_scale: Option<f64>,
    #[serde(default)]
    pub torque_constant_scale: Option<f64>,
    #[serde(default)]
    pub rms_error_rad: f64,
    #[serde(default)]
    pub source_log: String,
    #[serde(default)]
    pub fitted_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanarHint {
    pub normal: V3,
    #[serde(default)]
    pub origin: V3,
}

impl PhysicalModel {
    pub fn load(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        Self::parse(&text).map_err(|e| format!("{path}: {e}"))
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
    pub fn link(&self, name: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.name == name)
    }
    pub fn link_index(&self, name: &str) -> Option<usize> {
        self.links.iter().position(|l| l.name == name)
    }
    pub fn joint(&self, name: &str) -> Option<&Joint> {
        self.joints.iter().find(|j| j.name == name)
    }
    pub fn material_of(&self, link: &Link) -> Material {
        self.materials.get(&link.material).cloned().unwrap_or_default()
    }
    /// Kinetic friction coefficient between two materials (or "world").
    pub fn friction_between(&self, a: &str, b: &str) -> (f64, f64) {
        let look = |x: &str, y: &str| self.materials.get(x).and_then(|m| m.friction.get(y)).map(|f| (f.r#static, f.kinetic));
        look(a, b).or_else(|| look(b, a)).unwrap_or((0.5, 0.4))
    }
    /// The inferred values with any fitted identification applied.
    pub fn apply_identification(&mut self) {
        for j in &mut self.joints {
            if let Some(id) = self.identification.get(&j.name) {
                if let Some(f) = &id.friction {
                    j.physics.friction = f.clone();
                }
                if let Some(b) = id.backlash {
                    j.physics.backlash = b;
                }
                if let Some(s) = id.stiffness_scale {
                    j.physics.stiffness.radial *= s;
                    j.physics.stiffness.axial *= s;
                    j.physics.stiffness.bending *= s;
                }
                if let Some(s) = id.torque_constant_scale {
                    if let Some(m) = self.motors.iter_mut().find(|m| m.joint.as_deref() == Some(j.name.as_str())) {
                        m.electrical.torque_constant *= s;
                    }
                }
            }
        }
    }
}

// ---- handle store ---------------------------------------------------------
// Element factories only take scalar parameters, so the description is
// parked here and referenced by a numeric handle (`model = <handle>`).

static MODELS: OnceLock<Mutex<Vec<Arc<PhysicalModel>>>> = OnceLock::new();

/// Park a model; the returned handle is what `robot.articulated` reads.
pub fn register_model(model: PhysicalModel) -> f64 {
    let store = MODELS.get_or_init(|| Mutex::new(Vec::new()));
    let mut s = store.lock().unwrap_or_else(|p| p.into_inner());
    s.push(Arc::new(model));
    (s.len() - 1) as f64
}

pub fn model_by_handle(handle: f64) -> Option<Arc<PhysicalModel>> {
    let store = MODELS.get_or_init(|| Mutex::new(Vec::new()));
    let s = store.lock().unwrap_or_else(|p| p.into_inner());
    if !handle.is_finite() || handle < 0.0 || handle.fract() != 0.0 {
        return None;
    }
    s.get(handle as usize).cloned()
}
