//! Robot mode's composed robot (B4): a robot project's system run with its
//! controllers as they are and a jog block on the joints a person may move.
use super::{DriveLink, JOG_HOST, JogPlan, drive_hosts, jog_plan};
use crate::physical::{BuildOptions, PhysicalRobot};
use sim_core::{BehaviorRegistry, BlockImplementation, BlockInterface, BlockPort, BlockTiming, Checkpoint};
use sim_system::{BlockSource, Command, InstanceSpec, SystemDocument, Terminal};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Robot mode's jog block: each output is a joint's value in the shared
/// targets, in the robot's `targets_order` (`index`), a pure function of
/// that shared state.
struct JogBlock {
    targets: Arc<Mutex<Vec<f64>>>,
    index: Vec<usize>,
    interface: BlockInterface,
}

impl JogBlock {
    fn write(&self, outputs: &mut [f64]) {
        let held = self.targets.lock().unwrap_or_else(|p| p.into_inner());
        for (out, k) in outputs.iter_mut().zip(&self.index) {
            *out = held.get(*k).copied().unwrap_or(0.0);
        }
    }
}

impl BlockImplementation for JogBlock {
    fn label(&self) -> String {
        "Robot mode's joint jog".into()
    }
    fn interface(&self) -> BlockInterface {
        self.interface.clone()
    }
    fn initialize(&mut self, _t: f64, _period: f64, _inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.write(outputs);
        Ok(())
    }
    fn step(&mut self, _t: f64, _dt: f64, _inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.write(outputs);
        Ok(())
    }
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Stateless
    }
}

/// A robot project's system as Robot mode runs it: the robot measured inside
/// the composed system ([`PhysicalRobot::attach`], as the acceptance test
/// does), its controllers bound as they are, a jog block on the joints a
/// person may move, and the drive input (if the system has one) bound to
/// `drive`.
pub struct ComposedRobot {
    pub robot: PhysicalRobot,
    pub jog: JogPlan,
    /// The controller blocks that run, as `{block, implementation, period_s}`.
    pub controllers: Vec<serde_json::Value>,
    pub robot_instance: String,
}

/// Build [`ComposedRobot`] from the system file `document` in `base`.
pub fn compose_robot(document: &SystemDocument, base: &Path, registry: &BehaviorRegistry, drive: &DriveLink) -> Result<ComposedRobot, String> {
    let (robot_name, source, parameters, ports) = crate::acceptance::robot_of(document)?;
    let model_path = base.join(&source);
    let model_text = std::fs::read_to_string(&model_path).map_err(|e| format!("{}: {e}", model_path.display()))?;
    let model = sim_domain_robot::PhysicalModel::parse(&model_text).map_err(|e| format!("{}: {e}", model_path.display()))?;
    let jog = jog_plan(document, &robot_name, &ports, &model);
    let mut composed = document.clone();
    let period = model.control.period_s.max(1e-4);
    let mut timing = BlockTiming::periodic(period);
    timing.output_delay = (model.control.latency_s / period).round().max(0.0) as usize;
    let jog_name = (0..).map(|k| if k == 0 { "jog".to_owned() } else { format!("jog_{k}") }).find(|n| !composed.definitions[&composed.root].instances.contains_key(n)).expect("a free name");
    if !jog.channels.is_empty() {
        let interface = BlockInterface {
            inputs: Vec::new(),
            outputs: jog.channels.iter().map(|c| BlockPort::new(c.joint.clone(), c.kind.clone()).start(c.hold)).collect(),
            feedthrough: true,
        };
        let mut commands = vec![Command::AddInstance { at: String::new(), name: jog_name.clone(), instance: InstanceSpec::block(BlockSource::Host { name: JOG_HOST.into() }, interface, timing) }];
        commands.extend(jog.channels.iter().map(|c| Command::Connect { at: String::new(), terminals: vec![Terminal::port(&jog_name, &c.joint), c.terminal.clone()], label: String::new() }));
        sim_system::commands::apply(&mut composed, registry, &commands).map_err(|e| format!("the jog block does not fit the system: {e}"))?;
    }
    let flat = sim_system::flatten_with(&composed, registry, &crate::robot_generator::generators(base)).map_err(|e| e.to_string())?;
    let assembly = flat
        .generated_details
        .get(&robot_name)
        .and_then(|d| d.downcast_ref::<crate::robot_generator::Handle>())
        .and_then(|h| h.lock().unwrap_or_else(|p| p.into_inner()).take())
        .ok_or("the robot generator gave no assembly to measure")?;
    let config = crate::system_builder::config_for(document);
    let mut runtime = sim_compile::Runtime::new(flat.model.clone(), registry, config.integrator).map_err(|e| crate::system_builder::locate(&flat, format!("the system does not compile: {e}")))?;
    runtime.set_grid_clock(true);
    runtime.retry_halvings = 4;
    // The shared targets, in the robot's targets order: a jog writes them,
    // the jog block reads them at its ticks.
    let short = |n: &str| n.trim_start_matches("joint.").trim_start_matches("slide.").to_owned();
    let order: Vec<String> = assembly.targets_order.iter().map(|n| short(n)).collect();
    let mut initial: Vec<f64> = order.iter().map(|j| model.control.targets.get(j).copied().unwrap_or(0.0)).collect();
    let mut index = Vec::new();
    for c in &jog.channels {
        let k = match order.iter().position(|j| j == &c.joint) {
            Some(k) => k,
            None => {
                initial.push(c.hold);
                initial.len() - 1
            }
        };
        initial[k] = c.hold;
        index.push(k);
    }
    let targets = Arc::new(Mutex::new(initial));
    let mut hosts = drive_hosts(&runtime.model, Some(base), drive)?;
    {
        let (targets, index) = (targets.clone(), index.clone());
        hosts.insert(JOG_HOST.into(), Box::new(move |decl: &sim_core::BlockDecl| Ok(Box::new(JogBlock { targets: targets.clone(), index: index.clone(), interface: decl.interface.clone() }) as Box<dyn BlockImplementation>)));
    }
    crate::system_blocks::bind_with(&mut runtime, Some(base), &mut hosts)?;
    let controllers = runtime
        .model
        .blocks
        .iter()
        .filter(|b| b.name != jog_name)
        .map(|b| serde_json::json!({"block": b.name, "implementation": b.implementation.describe(), "period_s": b.timing.clock.nominal_period()}))
        .collect();
    let (opts, _) = crate::robot_generator::options("", &parameters)?;
    let mut robot = PhysicalRobot::attach(runtime, assembly, &BuildOptions { step: config.interval, ..opts })?;
    robot.targets = targets;
    robot.jog_refused = jog.refused.clone();
    Ok(ComposedRobot { robot, jog, controllers, robot_instance: robot_name })
}

/// The jog plan of the system file `document` (in `base`): its one robot
/// instance's name and which joints a person can move. Reads the robot's
/// model file; no build.
pub fn plan_for(document: &SystemDocument, base: &Path) -> Result<(String, JogPlan), String> {
    let (robot_name, source, _, ports) = crate::acceptance::robot_of(document)?;
    let model_path = base.join(&source);
    let text = std::fs::read_to_string(&model_path).map_err(|e| format!("{}: {e}", model_path.display()))?;
    let model = sim_domain_robot::PhysicalModel::parse(&text).map_err(|e| format!("{}: {e}", model_path.display()))?;
    let plan = jog_plan(document, &robot_name, &ports, &model);
    Ok((robot_name, plan))
}
