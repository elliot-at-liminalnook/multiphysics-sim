//! Display bindings derived from a compiled system: which parts turn or
//! slide with which simulated quantity, the moving pieces drawn inside
//! housings, ropes and belts, and every physical port's effort and flow for
//! the power, force, current and heat overlays. Display only: the physics
//! is the compiled model's; this decides what each observation moves.
use sim_inspect::animation::{
    AnimationDescription, ColorBinding, FlowBinding, FlowDomain, InternalBinding, InternalElement, Readout, RotationBinding, Tether,
    TranslationBinding,
};
use sim_inspect::spatial::{SpatialDescription, SpatialPart, SpatialShape};
use sim_inspect::{ObservationLocation, SystemDescription};
use std::collections::{BTreeMap, BTreeSet};

const ANGLE: &str = "sim.quantity.Angle";
const POSITION: &str = "sim.quantity.Length";

fn axis_of(part: &SpatialPart) -> [f32; 3] {
    sim_system::flatten::rotate(part.rotation_xyzw, [0., 1., 0.])
}
fn add(a: [f32; 3], b: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] + k * b[0], a[1] + k * b[1], a[2] + k * b[2]]
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(v: [f32; 3]) -> Option<[f32; 3]> {
    let n = dot(v, v).sqrt();
    (n > 1e-9).then(|| v.map(|x| x / n))
}
/// Radius and length of a part's display body along its axis.
fn body(part: &SpatialPart) -> (f32, f32) {
    match part.shape {
        SpatialShape::Cylinder { radius, length } => (radius, length),
        SpatialShape::Box { size } => (0.5 * size[0].min(size[2]), size[1]),
        SpatialShape::Sphere { radius } => (radius, 2. * radius),
    }
}
/// Gravity on a swinging link: drawn as the link itself.
fn is_link(component_type: &str) -> bool {
    component_type == "part.pendulum_gravity"
}
fn is_motor(component_type: &str) -> bool {
    component_type.contains("motor") && !component_type.contains("sensor")
}

/// Everything the viewers animate for a compiled system.
pub fn animation(description: &SystemDescription, spatial: &SpatialDescription) -> Option<AnimationDescription> {
    let parts: BTreeMap<&str, &SpatialPart> = spatial.parts.iter().map(|p| (p.component.as_str(), p)).collect();
    let kind = |component: &str| description.components.get(component).map(|c| c.component_type.as_str()).unwrap_or("");
    // Port → observables by quantity (across and through).
    let mut at_port: BTreeMap<&str, BTreeMap<&str, &str>> = BTreeMap::new();
    for (id, o) in &description.observables {
        if let ObservationLocation::Across { port, .. } | ObservationLocation::Through { port, .. } = &o.location {
            let available = o.availability == sim_inspect::Availability::Available || matches!(o.location, ObservationLocation::Through { .. });
            if available {
                at_port.entry(port.as_str()).or_default().insert(o.quantity.name.as_str(), id.as_str());
            }
        }
    }
    let net_of: BTreeMap<&str, &str> = description.nets.values().flat_map(|n| n.ports.iter().map(move |p| (p.as_str(), n.id.as_str()))).collect();
    let fixed = |port: &str| {
        net_of.get(port).is_some_and(|n| description.nets[*n].ports.iter().any(|p| kind(&description.ports[p].component).ends_with(".ground")))
    };
    let physical = |component: &str| description.ports.values().filter(|p| p.component == component && at_port.contains_key(p.id.as_str())).collect::<Vec<_>>();
    let quantity = |port: &str, q: &str| at_port.get(port).and_then(|m| m.get(q)).map(|s| s.to_string());
    let rotary = |component: &str| physical(component).into_iter().filter(|p| quantity(&p.id, ANGLE).is_some() && !fixed(&p.id)).collect::<Vec<_>>();
    let linear = |component: &str| physical(component).into_iter().filter(|p| quantity(&p.id, POSITION).is_some()).collect::<Vec<_>>();

    let mut colors = Vec::new();
    let mut rotations = Vec::new();
    let mut readouts = Vec::new();
    let mut internals = Vec::new();
    let mut translations = Vec::new();
    let mut tethers = Vec::new();
    let mut flows = Vec::new();

    // Whole parts: temperature colours; single-shaft parts (rotors,
    // flywheels, drums) spin; a part with several shafts is a housing and
    // shows its shafts as internal pieces instead. Converters (a rack's
    // pinion, a lead screw) never spin whole.
    for part in &spatial.parts {
        let c = part.component.as_str();
        if let Some(id) = physical(c).iter().find_map(|p| quantity(&p.id, "sim.quantity.Temperature")) {
            colors.push(ColorBinding { part: part.id.clone(), observable: id.clone(), range_kelvin: [293.15, 353.15], cold_srgb: [0.16, 0.60, 0.60], hot_srgb: [0.95, 0.22, 0.08] });
            readouts.push(Readout { label: format!("{} temperature", part.label), observable: id });
        }
        let all_angles = physical(c).into_iter().filter(|p| quantity(&p.id, ANGLE).is_some()).count();
        let r = rotary(c);
        if all_angles == 1 && r.len() == 1 && linear(c).is_empty() && !is_link(kind(c)) {
            let id = quantity(&r[0].id, ANGLE).unwrap();
            if sim_inspect::plot::unit(description, &description.observables[&id]) == "rad" {
                let marker = match part.shape {
                    SpatialShape::Cylinder { radius, .. } => Some(radius),
                    _ => None,
                };
                // A part on a swinging link's shaft (a sensor, a hub) rides the
                // link: it turns about the link's joint, not its own centre.
                let joint = net_of.get(r[0].id.as_str()).and_then(|n| {
                    description.nets[*n].ports.iter().map(|p| description.ports[p].component.as_str()).find(|q| *q != c && is_link(kind(q))).and_then(|q| parts.get(q))
                });
                let pivot = joint.map(|j| j.position).unwrap_or(part.position);
                let axis = joint.map(|j| axis_of(j)).unwrap_or(axis_of(part));
                rotations.push(RotationBinding { part: part.id.clone(), observable: id.clone(), axis, pivot, marker_radius: marker });
                readouts.push(Readout { label: format!("{} angle", part.label), observable: id });
            }
        }
    }
    let spinning: BTreeSet<String> = rotations.iter().map(|r| r.part.clone()).collect();
    // A net already shown turning by some whole part.
    let shown = |port: &str| {
        net_of.get(port).is_some_and(|n| description.nets[*n].ports.iter().any(|p| parts.get(description.ports[p].component.as_str()).is_some_and(|q| spinning.contains(&q.id))))
    };
    // Which end of a part a port's neighbours sit at (+1 along its axis, −1 against).
    let side = |part: &SpatialPart, port: &str| -> f32 {
        let axis = axis_of(part);
        let Some(net) = net_of.get(port) else { return 1. };
        let mut s = 0.;
        for p in &description.nets[*net].ports {
            let c = &description.ports[p].component;
            if c != &part.component {
                if let Some(q) = parts.get(c.as_str()) {
                    s += dot(sub(q.position, part.position), axis);
                }
            }
        }
        if s < 0. { -1. } else { 1. }
    };

    for part in &spatial.parts {
        let c = part.component.as_str();
        let t = kind(c);
        let axis = axis_of(part);
        let (radius, length) = body(part);
        let r = rotary(c);
        let push = |internals: &mut Vec<InternalBinding>, port: &str, gain: f64, element: InternalElement, center: [f32; 3], axis: [f32; 3], radius: f32, length: f32, color: [f32; 3]| {
            if let Some(id) = quantity(port, ANGLE) {
                internals.push(InternalBinding { part: part.id.clone(), observable: id, gain, element, center, axis, radius, length, color_srgb: color });
            }
        };
        if is_motor(t) {
            if let Some(shaft) = r.iter().find(|p| p.name == "shaft").or(r.first()) {
                // The armature inside the (drawn see-through) housing.
                push(&mut internals, &shaft.id, 1., InternalElement::Armature { poles: 3 }, part.position, axis, radius * 0.62, length * 0.62, [0.72, 0.45, 0.20]);
                if !shown(&shaft.id) {
                    let s = side(part, &shaft.id);
                    push(&mut internals, &shaft.id, 1., InternalElement::Shaft, add(part.position, axis, s * (0.5 * length + 0.12 * length)), axis, radius * 0.16, length * 0.3, [0.78, 0.80, 0.83]);
                }
            }
            continue;
        }
        if is_link(t) {
            // The link itself: a rod from the joint to its centre of mass, and a bob there.
            if let Some(shaft) = r.first() {
                let p = &description.components[c].parameters;
                let reach = p.get("arm").map(|v| v.value as f32).unwrap_or(0.1).max(0.005);
                let mass = p.get("mass").map(|v| v.value as f32).unwrap_or(0.5).max(0.01);
                let bob = (reach * 0.16 * mass.cbrt()).clamp(reach * 0.08, reach * 0.3);
                push(&mut internals, &shaft.id, 1., InternalElement::Arm, part.position, axis, bob, reach, [0.36, 0.55, 0.78]);
            }
            continue;
        }
        if spinning.contains(&part.id) || r.is_empty() {
            continue;
        }
        let ratio = description.components[c].parameters.get("ratio").map(|v| v.value);
        match (t, r.as_slice()) {
            ("rotational.spring" | "rotational.damper" | "rotational.backlash_mesh", [a, b]) => {
                // Two coupling halves: the twist between them is the spring's.
                for p in [a, b] {
                    let s = side(part, &p.id);
                    push(&mut internals, &p.id, 1., InternalElement::CouplingHalf { lugs: 2 }, add(part.position, axis, s * 0.25 * length), axis, radius * 0.95, length * 0.46, [0.55, 0.58, 0.62]);
                }
            }
            ("rotational.ideal_gear" | "rotational.lossy_gear", [_, _]) if ratio.is_some_and(|k| (1. ..=5.).contains(&k.abs())) => {
                // input = ratio × output, both turning the same way: pinion,
                // idler and gear. The idler's angle is pure kinematics.
                let k = ratio.unwrap().abs() as f32;
                let input = r.iter().find(|p| p.name == "input").unwrap_or(&r[0]);
                let output = r.iter().find(|p| p.name == "output").unwrap_or(&r[1]);
                let pitch = (radius.max(0.004)) * 0.9;
                let (rp, ri, rg) = (pitch, pitch, pitch * k);
                let across = unit(cross(axis, if axis[1].abs() < 0.9 { [0., 1., 0.] } else { [1., 0., 0.] })).unwrap_or([1., 0., 0.]);
                let idler = add(part.position, across, rp + ri);
                let gear = add(idler, across, ri + rg);
                let teeth = |r: f32| ((r / pitch) * 12.).round().max(8.) as u32;
                push(&mut internals, &input.id, 1., InternalElement::Gear { teeth: teeth(rp) }, part.position, axis, rp, pitch * 0.8, [0.74, 0.76, 0.80]);
                push(&mut internals, &input.id, -(rp / ri) as f64, InternalElement::Gear { teeth: teeth(ri) }, idler, axis, ri, pitch * 0.8, [0.60, 0.63, 0.68]);
                push(&mut internals, &output.id, 1., InternalElement::Gear { teeth: teeth(rg) }, gear, axis, rg, pitch * 0.8, [0.78, 0.62, 0.30]);
            }
            _ => {
                // Any other housing with turning shafts: a shaft end per port.
                for p in r.iter().filter(|p| !shown(&p.id)) {
                    let s = side(part, &p.id);
                    push(&mut internals, &p.id, 1., InternalElement::Shaft, add(part.position, axis, s * 0.5 * length), axis, radius.max(0.002) * 0.35, length.max(0.004) * 0.5, [0.78, 0.80, 0.83]);
                }
            }
        }
    }

    // Parts on a single moving linear port slide with its position.
    for part in &spatial.parts {
        let c = part.component.as_str();
        let ports = physical(c);
        let lin = linear(c);
        // End stops are fixed bumpers acting on the node, not riders on it.
        if ports.len() == 1 && lin.len() == 1 && !fixed(&lin[0].id) && !kind(c).contains("stop") {
            let id = quantity(&lin[0].id, POSITION).unwrap();
            translations.push(TranslationBinding { part: part.id.clone(), observable: id, axis: axis_of(part), reference_m: 0. });
        }
    }
    // A wheel rolls: it turns with its axle and rides with the vehicle it
    // pushes, and so does the rest of its drive module (the parts in the same
    // subsystem: motor, gearbox, hub).
    for part in &spatial.parts {
        let c = part.component.as_str();
        if kind(c) != "part.drive_wheel" {
            continue;
        }
        let (Some(axle), Some(chassis)) = (rotary(c).into_iter().find(|p| p.name == "axle"), linear(c).into_iter().find(|p| p.name == "chassis")) else { continue };
        let Some(position) = quantity(&chassis.id, POSITION) else { continue };
        let vehicle = net_of.get(chassis.id.as_str()).and_then(|n| description.nets[*n].ports.iter().map(|p| description.ports[p].component.as_str()).find(|q| kind(q) == "translational.mass").and_then(|q| parts.get(q)));
        let travel = vehicle.map(|v| axis_of(v)).unwrap_or([1., 0., 0.]);
        if !rotations.iter().any(|r| r.part == part.id) {
            let id = quantity(&axle.id, ANGLE).unwrap();
            let marker = match part.shape {
                SpatialShape::Cylinder { radius, .. } => Some(radius),
                _ => None,
            };
            rotations.push(RotationBinding { part: part.id.clone(), observable: id, axis: axis_of(part), pivot: part.position, marker_radius: marker });
        }
        let module = c.rsplit_once('/').map(|(m, _)| m);
        for q in &spatial.parts {
            let same = q.component == c || (module.is_some() && q.component.rsplit_once('/').map(|(m, _)| m) == module);
            if same && !translations.iter().any(|t| t.part == q.id) {
                translations.push(TranslationBinding { part: q.id.clone(), observable: position.clone(), axis: travel, reference_m: 0. });
            }
        }
    }
    // Ropes and belts: from the drum or pulley's rim to what they pull.
    let sliding: BTreeMap<&str, &TranslationBinding> = translations.iter().map(|t| (t.part.as_str(), t)).collect();
    for (id, comp) in &description.components {
        if !matches!(comp.component_type.as_str(), "part.rack_pinion" | "part.timing_belt") {
            continue;
        }
        let (Some(rot), Some(lin)) = (physical(id).into_iter().find(|p| quantity(&p.id, ANGLE).is_some()), linear(id).first().copied()) else { continue };
        let on = |port: &str| net_of.get(port).map(|n| description.nets[*n].ports.iter().filter_map(|p| parts.get(description.ports[p].component.as_str()).copied()).collect::<Vec<_>>()).unwrap_or_default();
        let Some(drum) = on(&rot.id).into_iter().filter(|p| spinning.contains(&p.id)).max_by(|a, b| body(a).0.total_cmp(&body(b).0)) else { continue };
        // What the rope carries: a mass if there is one, else the largest rider.
        let riders: Vec<&SpatialPart> = on(&lin.id).into_iter().filter(|p| sliding.contains_key(p.id.as_str())).collect();
        let Some(load) = riders.iter().find(|p| kind(&p.component) == "translational.mass").or_else(|| riders.iter().max_by(|a, b| body(a).0.total_cmp(&body(b).0))).copied() else { continue };
        let r = comp.parameters.get("radius").map(|v| v.value as f32).unwrap_or(body(drum).0);
        let axis = axis_of(drum);
        let travel = sliding[load.id.as_str()].axis;
        let v = sub(load.position, drum.position);
        let off_axis = add(v, axis, -dot(v, axis));
        let lateral = add(off_axis, travel, -dot(off_axis, travel));
        let dir = unit(lateral).or(unit(off_axis)).unwrap_or([1., 0., 0.]);
        let top = match load.shape {
            SpatialShape::Box { size } => size[1] * 0.5,
            SpatialShape::Cylinder { length, .. } => length * 0.5,
            SpatialShape::Sphere { radius } => radius,
        };
        tethers.push(Tether { anchor: add(drum.position, dir, r), part: load.id.clone(), attach: [0., top, 0.] });
    }

    // Every physical port with a flow: power, arrows, current and heat.
    for port in description.ports.values() {
        let Some(part) = parts.get(port.component.as_str()) else { continue };
        let Some(net) = net_of.get(port.id.as_str()) else { continue };
        let domain = [
            (FlowDomain::Electrical, "sim.quantity.Voltage", "sim.quantity.Current"),
            (FlowDomain::Rotational, "sim.quantity.AngularVelocity", "sim.quantity.Torque"),
            (FlowDomain::Translational, "sim.quantity.LinearVelocity", "sim.quantity.Force"),
            (FlowDomain::Thermal, "sim.quantity.Temperature", "sim.quantity.HeatFlow"),
        ]
        .into_iter()
        .find_map(|(d, e, f)| quantity(&port.id, f).map(|flow| (d, quantity(&port.id, e), flow)));
        if let Some((domain, effort, flow)) = domain {
            flows.push(FlowBinding { part: part.id.clone(), component: port.component.clone(), port: port.name.clone(), net: net.to_string(), domain, effort, flow: Some(flow) });
        }
    }

    readouts.truncate(16);
    let animation = AnimationDescription {
        version: 1,
        description_id: description.id.clone(),
        provenance: "Generated by the system builder from the compiled model: shaft angles turn parts and their internal pieces (an idler's angle is gear kinematics), positions slide parts, and each port's effort and flow drive the power, force, current and heat overlays. Temperatures colour parts on an illustrative 293–353 K scale, not a limit.".into(),
        rotations,
        colors,
        readouts,
        internals,
        translations,
        tethers,
        flows,
    };
    animation.validate(description, spatial).ok().map(|_| animation)
}
