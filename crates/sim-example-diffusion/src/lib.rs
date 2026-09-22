//! Example extension: well-mixed volumes coupled by linear species diffusion.
//! Concentration is mol/m³; signed flow into an element is mol/s. This model
//! makes no energy/entropy claim because concentration alone is not a potential.
use sim_core::definitions::*;
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, ModelWorld,
    ParameterDeclaration as P, QuantityKind, RegistryError, StateDeclaration, acausal, param,
};
use std::collections::BTreeMap;

pub const CONCENTRATION: QuantityKind =
    QuantityKind::named("example.diffusion.concentration", 1, "mol/m³");
pub const SPECIES: ConnectorKind = ConnectorKind::named("example.diffusion.species", 1);
pub const STORAGE: &str = "example.diffusion.storage";
pub const CONDUCTANCE: &str = "example.diffusion.conductance";
pub const BOUNDARY: &str = "example.diffusion.boundary";

pub struct Concentration;
impl QuantityDefinition for Concentration {
    fn descriptor(&self) -> QuantityDescriptor {
        QuantityDescriptor {
            id: CONCENTRATION.definition_id(),
            label: "Species concentration".into(),
            canonical_unit: "mol/m³".into(),
            dimension: Dimension::si([-3, 0, 0, 0, 0, 1, 0]).into(),
            nature: QuantityNature::Linear,
            display_units: vec![],
        }
    }
}
pub struct Species;
impl ConnectorDefinition for Species {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            id: SPECIES.definition_id(),
            label: "Species diffusion".into(),
            lanes: vec![LaneDescriptor {
                across: VariableDescriptor {
                    name: "concentration".into(),
                    quantity: CONCENTRATION.definition_id(),
                },
                through: Some(VariableDescriptor {
                    name: "molar_flow".into(),
                    quantity: QuantityKind::MolarFlow.definition_id(),
                }),
                derivative_of: None,
            }],
            rule: ConnectionRule::Balanced,
            energy: PortEnergy::Unavailable,
        }
    }
}

pub struct Storage {
    pub volume: f64,
}
impl Behavior for Storage {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.add_through(0, self.volume * ctx.across_rate(0));
    }
}
pub struct Conductance {
    pub conductance: f64,
}
impl Behavior for Conductance {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![]
    }
    fn residual(&self, ctx: &mut Context) {
        let flow = self.conductance * (ctx.across(0) - ctx.across(1));
        ctx.add_through(0, flow);
        ctx.add_through(1, -flow);
    }
}
pub struct Boundary {
    pub concentration: f64,
}
impl Behavior for Boundary {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new(
            "absorbed",
            QuantityKind::MolarFlow,
            0.,
        )]
    }
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        vec![(0, 0, self.concentration)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across(0) - self.concentration);
        ctx.add_through(0, ctx.state(0));
    }
}
type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;
fn storage(p: &Params) -> Made {
    Ok(Box::new(Storage {
        volume: param(p, "volume")?,
    }))
}
fn conductance(p: &Params) -> Made {
    Ok(Box::new(Conductance {
        conductance: param(p, "conductance")?,
    }))
}
fn boundary(p: &Params) -> Made {
    Ok(Box::new(Boundary {
        concentration: param(p, "concentration")?,
    }))
}

/// Explicit host wiring, with no changes to core domain lists or compiler cases.
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register_quantity(&Concentration)?;
    registry.register_connector(&Species)?;
    for d in [
        BehaviorDescriptor::new(
            STORAGE,
            "Well-mixed volume",
            vec![acausal("species", SPECIES)],
            storage,
        )
        .with_parameters(vec![P::required("volume", "m³").positive()]),
        BehaviorDescriptor::new(
            CONDUCTANCE,
            "Diffusion path",
            vec![acausal("a", SPECIES), acausal("b", SPECIES)],
            conductance,
        )
        .with_parameters(vec![P::required("conductance", "m³/s").positive()]),
        BehaviorDescriptor::new(
            BOUNDARY,
            "Fixed concentration",
            vec![acausal("species", SPECIES)],
            boundary,
        )
        .with_parameters(vec![P::required("concentration", "mol/m³").nonnegative()]),
    ] {
        registry.register(d)?;
    }
    Ok(())
}

/// Illustrative SI parameters: V=2 m³, G=0.5 m³/s, c0=3, boundary=1 mol/m³.
pub fn relaxation(registry: &BehaviorRegistry) -> Result<ModelWorld, RegistryError> {
    let mut model = ModelWorld::default();
    let tank = model.part(
        registry,
        "Storage volume",
        STORAGE,
        [("volume", 2.), ("initial.concentration", 3.)],
    )?;
    let path = model.part(
        registry,
        "Diffusion path",
        CONDUCTANCE,
        [("conductance", 0.5)],
    )?;
    let reservoir = model.part(registry, "Reservoir", BOUNDARY, [("concentration", 1.)])?;
    model.connect([tank.port("species"), path.port("a")]);
    model.connect([path.port("b"), reservoir.port("species")]);
    Ok(model)
}
