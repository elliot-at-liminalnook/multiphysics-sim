//! A running FMU instance as a block implementation (FMI 3 Co-Simulation,
//! fixed communication step = the block's clock interval, no event mode).
use crate::abi::{self, Binary, fmi3Instance, fmi3Status};
use crate::description::ValueType;
use crate::fmu::{Binding, Fmu};
use crate::FmiError;
use sim_core::{BlockImplementation, BlockInterface, Checkpoint};
use std::collections::BTreeSet;
use std::ffi::{CString, c_char, c_void};
use std::sync::Mutex;

/// Instantiation tokens of FMUs that may be instantiated only once per
/// process and have a live instance.
static ONCE: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

struct OnceGuard(String);
impl Drop for OnceGuard {
    fn drop(&mut self) {
        ONCE.lock().unwrap_or_else(|p| p.into_inner()).remove(&self.0);
    }
}

/// Messages the FMU logged (newest last, at most `LOG_KEEP`).
type Log = Mutex<Vec<String>>;
const LOG_KEEP: usize = 32;

unsafe extern "C" fn log_message(env: *mut c_void, status: fmi3Status, category: *const c_char, message: *const c_char) {
    if env.is_null() {
        return;
    }
    let text = |p: *const c_char| if p.is_null() { String::new() } else { unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned() };
    let log = unsafe { &*(env as *const Log) };
    let mut log = log.lock().unwrap_or_else(|p| p.into_inner());
    log.push(format!("{} [{}] {}", abi::status_name(status), text(category), text(message)));
    if log.len() > LOG_KEEP {
        let excess = log.len() - LOG_KEEP;
        log.drain(..excess);
    }
}

/// One FMU instance. Owns its binary handle; the instance is freed before
/// the library is released.
pub struct FmuBlock {
    label: String,
    interface: BlockInterface,
    binding: Binding,
    binary: Binary,
    instance: fmi3Instance,
    // Boxed: the FMU holds this address as its instance environment.
    log: Box<Log>,
    initialized: bool,
    terminated: bool,
    fatal: bool,
    checkpoints: bool,
    _once: Option<OnceGuard>,
    // Last: the extracted archive (the binary and `resources/`) outlives
    // the instance and the library handle.
    _extraction: std::sync::Arc<tempfile::TempDir>,
}

// The FMU instance is only ever called through `&mut self` (or `&self`
// under the scheduler's lock), one call at a time: FMI 3 instances may be
// used from any thread as long as calls are not concurrent.
unsafe impl Send for FmuBlock {}

impl FmuBlock {
    /// Instantiate the FMU for block `name` with its checked `binding`, and
    /// set the parameters. Refuses a second live instance of an FMU that
    /// can be instantiated only once per process.
    pub fn new(fmu: &Fmu, name: &str, interface: BlockInterface, binding: Binding) -> Result<Self, FmiError> {
        let fail = |m: String| FmiError::Instance(format!("block `{name}` ({}): {m}", fmu.path.display()));
        let cs = fmu.description.co_simulation.clone().expect("checked at load");
        let once = if cs.can_be_instantiated_only_once_per_process {
            let token = fmu.description.instantiation_token.clone();
            let mut live = ONCE.lock().unwrap_or_else(|p| p.into_inner());
            if !live.insert(token.clone()) {
                return Err(fail("the FMU can be instantiated only once per process (canBeInstantiatedOnlyOncePerProcess) and already has a live instance".into()));
            }
            Some(OnceGuard(token))
        } else {
            None
        };
        let binary = unsafe { Binary::load(&fmu.binary_path()) }.map_err(&fail)?;
        let version = binary.version();
        if !version.starts_with("3.") {
            return Err(fail(format!("the binary reports FMI version `{version}`, not 3.x")));
        }
        let log: Box<Log> = Box::new(Mutex::new(Vec::new()));
        let c = |s: &str| CString::new(s).map_err(|_| fail(format!("`{s}` contains a NUL byte")));
        let (instance_name, token, resources) = (c(name)?, c(&fmu.description.instantiation_token)?, c(&fmu.resource_path())?);
        let instance = unsafe {
            (binary.instantiate)(
                instance_name.as_ptr(),
                token.as_ptr(),
                resources.as_ptr(),
                false,
                true,
                false,
                false,
                std::ptr::null(),
                0,
                &*log as *const Log as *mut c_void,
                Some(log_message),
                None,
            )
        };
        let checkpoints = cs.can_get_and_set_fmu_state && cs.can_serialize_fmu_state && binary.state.is_some();
        let mut block = Self {
            label: format!("FMU {} ({})", fmu.description.model_name, fmu.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
            interface,
            binding,
            binary,
            instance,
            log,
            initialized: false,
            terminated: false,
            fatal: false,
            checkpoints,
            _once: once,
            _extraction: fmu.extraction(),
        };
        if block.instance.is_null() {
            return Err(fail(format!("fmi3InstantiateCoSimulation failed{}", block.log_tail())));
        }
        block.set_parameters().map_err(fail)?;
        Ok(block)
    }

    fn log_tail(&self) -> String {
        let log = self.log.lock().unwrap_or_else(|p| p.into_inner());
        if log.is_empty() { String::new() } else { format!(" (FMU log: {})", log.iter().rev().take(4).rev().cloned().collect::<Vec<_>>().join(" | ")) }
    }

    fn check(&mut self, what: &str, status: fmi3Status) -> Result<(), String> {
        match status {
            abi::OK | abi::WARNING => Ok(()),
            abi::FATAL => {
                self.fatal = true;
                Err(format!("{what} returned fmi3Fatal: the FMU cannot continue{}", self.log_tail()))
            }
            other => Err(format!("{what} returned {}{}", abi::status_name(other), self.log_tail())),
        }
    }

    fn set_parameters(&mut self) -> Result<(), String> {
        for (vr, ty, value) in self.binding.parameters.clone() {
            self.set_one(vr, ty, value).map_err(|e| format!("setting parameter (value reference {vr}) = {value}: {e}"))?;
        }
        Ok(())
    }

    fn set_one(&mut self, vr: u32, ty: ValueType, value: f64) -> Result<(), String> {
        macro_rules! set_int {
            ($field:ident, $t:ty) => {{
                let access = self.binary.$field.as_ref().ok_or(concat!("the binary lacks the setter for ", stringify!($t)))?;
                let rounded = value.round();
                if !value.is_finite() || rounded < <$t>::MIN as f64 || rounded > <$t>::MAX as f64 {
                    return Err(format!("{value} does not fit the FMU's {} variable", stringify!($t)));
                }
                let v = rounded as $t;
                unsafe { (access.set)(self.instance, &vr, 1, &v, 1) }
            }};
        }
        let status = match ty {
            ValueType::Float64 => {
                let a = self.binary.float64.as_ref().ok_or("the binary lacks fmi3SetFloat64")?;
                unsafe { (a.set)(self.instance, &vr, 1, &value, 1) }
            }
            ValueType::Float32 => {
                let a = self.binary.float32.as_ref().ok_or("the binary lacks fmi3SetFloat32")?;
                let v = value as f32;
                unsafe { (a.set)(self.instance, &vr, 1, &v, 1) }
            }
            ValueType::Boolean => {
                let a = self.binary.boolean.as_ref().ok_or("the binary lacks fmi3SetBoolean")?;
                // A signal is true at 0.5 and above.
                let v = value >= 0.5;
                unsafe { (a.set)(self.instance, &vr, 1, &v, 1) }
            }
            ValueType::Int8 => set_int!(int8, i8),
            ValueType::UInt8 => set_int!(uint8, u8),
            ValueType::Int16 => set_int!(int16, i16),
            ValueType::UInt16 => set_int!(uint16, u16),
            ValueType::Int32 => set_int!(int32, i32),
            ValueType::UInt32 => set_int!(uint32, u32),
            ValueType::Int64 => set_int!(int64, i64),
            ValueType::UInt64 => set_int!(uint64, u64),
            other => return Err(format!("{other:?} values are not supported")),
        };
        self.check("fmi3Set", status)
    }

    fn get_one(&mut self, vr: u32, ty: ValueType) -> Result<f64, String> {
        macro_rules! get {
            ($field:ident, $t:ty, $zero:expr) => {{
                let access = self.binary.$field.as_ref().ok_or(concat!("the binary lacks the getter for ", stringify!($t)))?;
                let mut v: $t = $zero;
                let status = unsafe { (access.get)(self.instance, &vr, 1, &mut v, 1) };
                self.check("fmi3Get", status)?;
                v
            }};
        }
        Ok(match ty {
            ValueType::Float64 => get!(float64, f64, 0.0),
            ValueType::Float32 => get!(float32, f32, 0.0) as f64,
            ValueType::Boolean => if get!(boolean, bool, false) { 1.0 } else { 0.0 },
            ValueType::Int8 => get!(int8, i8, 0) as f64,
            ValueType::UInt8 => get!(uint8, u8, 0) as f64,
            ValueType::Int16 => get!(int16, i16, 0) as f64,
            ValueType::UInt16 => get!(uint16, u16, 0) as f64,
            ValueType::Int32 => get!(int32, i32, 0) as f64,
            ValueType::UInt32 => get!(uint32, u32, 0) as f64,
            ValueType::Int64 => get!(int64, i64, 0) as f64,
            ValueType::UInt64 => get!(uint64, u64, 0) as f64,
            other => return Err(format!("{other:?} values are not supported")),
        })
    }

    fn set_inputs(&mut self, inputs: &[f64]) -> Result<(), String> {
        for (k, (vr, ty)) in self.binding.inputs.clone().into_iter().enumerate() {
            self.set_one(vr, ty, inputs[k]).map_err(|e| format!("input `{}`: {e}", self.interface.inputs[k].name))?;
        }
        Ok(())
    }

    fn get_outputs(&mut self, outputs: &mut [f64]) -> Result<(), String> {
        for (k, (vr, ty)) in self.binding.outputs.clone().into_iter().enumerate() {
            outputs[k] = self.get_one(vr, ty).map_err(|e| format!("output `{}`: {e}", self.interface.outputs[k].name))?;
        }
        Ok(())
    }

    fn usable(&self) -> Result<(), String> {
        if self.fatal {
            return Err("the FMU returned fmi3Fatal earlier; it cannot be called again".into());
        }
        if self.terminated {
            return Err("the FMU instance was terminated".into());
        }
        Ok(())
    }
}

impl BlockImplementation for FmuBlock {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn interface(&self) -> BlockInterface {
        self.interface.clone()
    }

    fn initialize(&mut self, t: f64, _dt: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.usable()?;
        let status = unsafe { (self.binary.enter_initialization_mode)(self.instance, false, 0.0, t, false, 0.0) };
        self.check("fmi3EnterInitializationMode", status)?;
        self.set_inputs(inputs)?;
        let status = unsafe { (self.binary.exit_initialization_mode)(self.instance) };
        self.check("fmi3ExitInitializationMode", status)?;
        self.initialized = true;
        self.get_outputs(outputs)
    }

    fn step(&mut self, t: f64, dt: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.usable()?;
        self.set_inputs(inputs)?;
        let (mut event, mut terminate, mut early, mut last) = (false, false, false, t);
        let status = unsafe { (self.binary.do_step)(self.instance, t, dt, true, &mut event, &mut terminate, &mut early, &mut last) };
        self.check(&format!("fmi3DoStep(t = {t}, h = {dt})"), status)?;
        if terminate {
            return Err(format!("the FMU asked to terminate the simulation at t = {last}{}", self.log_tail()));
        }
        if early {
            return Err(format!("the FMU returned early from its step (at t = {last}), which this importer does not allow"));
        }
        if event {
            return Err("the FMU requested event handling, which needs event mode (not supported)".into());
        }
        self.get_outputs(outputs)
    }

    fn terminate(&mut self) {
        if self.initialized && !self.terminated && !self.fatal {
            let status = unsafe { (self.binary.terminate)(self.instance) };
            let _ = self.check("fmi3Terminate", status);
        }
        self.terminated = true;
    }

    fn checkpoint(&self) -> Checkpoint {
        if !self.checkpoints {
            return Checkpoint::Unsupported(format!("{} cannot save its state (canGetAndSetFMUState and canSerializeFMUState are not both true)", self.label));
        }
        if !self.initialized {
            return Checkpoint::State(vec![0]);
        }
        let f = self.binary.state.as_ref().expect("checked");
        let mut state: abi::fmi3FMUState = std::ptr::null_mut();
        let mut bytes = vec![1u8];
        unsafe {
            if (f.get)(self.instance, &mut state) > abi::WARNING {
                return Checkpoint::Unsupported(format!("{}: fmi3GetFMUState failed{}", self.label, self.log_tail()));
            }
            let mut size = 0usize;
            let ok = (f.size)(self.instance, state, &mut size) <= abi::WARNING && {
                bytes.resize(1 + size, 0);
                (f.serialize)(self.instance, state, bytes[1..].as_mut_ptr(), size) <= abi::WARNING
            };
            (f.free)(self.instance, &mut state);
            if !ok {
                return Checkpoint::Unsupported(format!("{}: serializing its state failed{}", self.label, self.log_tail()));
            }
        }
        Checkpoint::State(bytes)
    }

    fn restore(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.usable()?;
        if !self.checkpoints {
            return Err(format!("{} cannot restore a state", self.label));
        }
        match bytes.split_first() {
            Some((0, _)) => {
                if self.initialized {
                    let reset = self.binary.reset.ok_or("the binary lacks fmi3Reset")?;
                    let status = unsafe { reset(self.instance) };
                    self.check("fmi3Reset", status)?;
                    self.initialized = false;
                    self.set_parameters()?;
                }
                Ok(())
            }
            Some((1, serialized)) => {
                let f = self.binary.state.as_ref().expect("checked");
                let (deserialize, set, free) = (f.deserialize, f.set, f.free);
                let mut state: abi::fmi3FMUState = std::ptr::null_mut();
                let status = unsafe { deserialize(self.instance, serialized.as_ptr(), serialized.len(), &mut state) };
                self.check("fmi3DeserializeFMUState", status)?;
                let status = unsafe { set(self.instance, state) };
                unsafe { free(self.instance, &mut state) };
                self.check("fmi3SetFMUState", status)?;
                self.initialized = true;
                Ok(())
            }
            _ => Err("not a state this block saved".into()),
        }
    }
}

impl Drop for FmuBlock {
    fn drop(&mut self) {
        self.terminate();
        // After fmi3Fatal no function may be called, fmi3FreeInstance included.
        if !self.fatal && !self.instance.is_null() {
            unsafe { (self.binary.free_instance)(self.instance) };
            self.instance = std::ptr::null_mut();
        }
    }
}
