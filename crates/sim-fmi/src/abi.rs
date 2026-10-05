//! The FMI 3.0 C ABI this importer calls, written from the standard's
//! headers (`fmi3PlatformTypes.h`, `fmi3FunctionTypes.h`; vendored under
//! examples/fmi/include, 2-clause BSD). Only Co-Simulation and the
//! functions the block needs; every other entry point is never looked up.
#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_void};

pub type fmi3Instance = *mut c_void;
pub type fmi3InstanceEnvironment = *mut c_void;
pub type fmi3FMUState = *mut c_void;
pub type fmi3ValueReference = u32;
/// `fmi3Status`: a C enum.
pub type fmi3Status = i32;

pub const OK: fmi3Status = 0;
pub const WARNING: fmi3Status = 1;
pub const DISCARD: fmi3Status = 2;
pub const ERROR: fmi3Status = 3;
pub const FATAL: fmi3Status = 4;

pub fn status_name(status: fmi3Status) -> &'static str {
    match status {
        OK => "fmi3OK",
        WARNING => "fmi3Warning",
        DISCARD => "fmi3Discard",
        ERROR => "fmi3Error",
        FATAL => "fmi3Fatal",
        _ => "an unknown status",
    }
}

pub type fmi3LogMessageCallback = unsafe extern "C" fn(fmi3InstanceEnvironment, fmi3Status, *const c_char, *const c_char);
/// Never used (intermediate update is not supported): passed as null.
pub type fmi3IntermediateUpdateCallback = unsafe extern "C" fn();

pub type GetVersion = unsafe extern "C" fn() -> *const c_char;
pub type InstantiateCoSimulation = unsafe extern "C" fn(
    instance_name: *const c_char,
    instantiation_token: *const c_char,
    resource_path: *const c_char,
    visible: bool,
    logging_on: bool,
    event_mode_used: bool,
    early_return_allowed: bool,
    required_intermediate_variables: *const fmi3ValueReference,
    n_required_intermediate_variables: usize,
    instance_environment: fmi3InstanceEnvironment,
    log_message: Option<fmi3LogMessageCallback>,
    intermediate_update: Option<fmi3IntermediateUpdateCallback>,
) -> fmi3Instance;
pub type FreeInstance = unsafe extern "C" fn(fmi3Instance);
pub type EnterInitializationMode = unsafe extern "C" fn(fmi3Instance, bool, f64, f64, bool, f64) -> fmi3Status;
pub type InstanceOnly = unsafe extern "C" fn(fmi3Instance) -> fmi3Status;
pub type DoStep = unsafe extern "C" fn(fmi3Instance, f64, f64, bool, *mut bool, *mut bool, *mut bool, *mut f64) -> fmi3Status;
pub type Get<T> = unsafe extern "C" fn(fmi3Instance, *const fmi3ValueReference, usize, *mut T, usize) -> fmi3Status;
pub type Set<T> = unsafe extern "C" fn(fmi3Instance, *const fmi3ValueReference, usize, *const T, usize) -> fmi3Status;
pub type GetFMUState = unsafe extern "C" fn(fmi3Instance, *mut fmi3FMUState) -> fmi3Status;
pub type SetFMUState = unsafe extern "C" fn(fmi3Instance, fmi3FMUState) -> fmi3Status;
pub type FreeFMUState = unsafe extern "C" fn(fmi3Instance, *mut fmi3FMUState) -> fmi3Status;
pub type SerializedFMUStateSize = unsafe extern "C" fn(fmi3Instance, fmi3FMUState, *mut usize) -> fmi3Status;
pub type SerializeFMUState = unsafe extern "C" fn(fmi3Instance, fmi3FMUState, *mut u8, usize) -> fmi3Status;
pub type DeserializeFMUState = unsafe extern "C" fn(fmi3Instance, *const u8, usize, *mut fmi3FMUState) -> fmi3Status;

/// Getter and setter of one FMI value type.
pub struct Access<T> {
    pub get: Get<T>,
    pub set: Set<T>,
}

/// The FMU's shared library with the entry points the block calls. The
/// library stays loaded while this lives; instances must be freed first.
pub struct Binary {
    pub get_version: GetVersion,
    pub instantiate: InstantiateCoSimulation,
    pub free_instance: FreeInstance,
    pub enter_initialization_mode: EnterInitializationMode,
    pub exit_initialization_mode: InstanceOnly,
    pub terminate: InstanceOnly,
    pub reset: Option<InstanceOnly>,
    pub do_step: DoStep,
    pub float64: Option<Access<f64>>,
    pub float32: Option<Access<f32>>,
    pub int8: Option<Access<i8>>,
    pub uint8: Option<Access<u8>>,
    pub int16: Option<Access<i16>>,
    pub uint16: Option<Access<u16>>,
    pub int32: Option<Access<i32>>,
    pub uint32: Option<Access<u32>>,
    pub int64: Option<Access<i64>>,
    pub uint64: Option<Access<u64>>,
    pub boolean: Option<Access<bool>>,
    pub state: Option<StateFunctions>,
    // Last: unloaded after every pointer above is gone.
    _library: libloading::Library,
}

pub struct StateFunctions {
    pub get: GetFMUState,
    pub set: SetFMUState,
    pub free: FreeFMUState,
    pub size: SerializedFMUStateSize,
    pub serialize: SerializeFMUState,
    pub deserialize: DeserializeFMUState,
}

impl Binary {
    /// Load the library and resolve the entry points. A missing required
    /// function is an error naming it.
    ///
    /// # Safety
    /// Loading runs the library's initialisers: the file must be the FMU's
    /// own binary (the caller extracted it from the archive it validated).
    pub unsafe fn load(path: &std::path::Path) -> Result<Self, String> {
        let library = unsafe { libloading::Library::new(path) }.map_err(|e| format!("cannot load the FMU's binary {}: {e}", path.display()))?;
        unsafe fn symbol<T: Copy>(library: &libloading::Library, name: &str) -> Option<T> {
            unsafe { library.get::<T>(name.as_bytes()).ok().map(|s| *s) }
        }
        let required = |name: &str| format!("the FMU's binary does not export `{name}`, which FMI 3 Co-Simulation requires");
        macro_rules! req {
            ($name:literal) => {
                unsafe { symbol(&library, $name) }.ok_or_else(|| required($name))?
            };
        }
        macro_rules! access {
            ($get:literal, $set:literal) => {
                match (unsafe { symbol(&library, $get) }, unsafe { symbol(&library, $set) }) {
                    (Some(get), Some(set)) => Some(Access { get, set }),
                    _ => None,
                }
            };
        }
        let state = (|| {
            Some(StateFunctions {
                get: unsafe { symbol(&library, "fmi3GetFMUState") }?,
                set: unsafe { symbol(&library, "fmi3SetFMUState") }?,
                free: unsafe { symbol(&library, "fmi3FreeFMUState") }?,
                size: unsafe { symbol(&library, "fmi3SerializedFMUStateSize") }?,
                serialize: unsafe { symbol(&library, "fmi3SerializeFMUState") }?,
                deserialize: unsafe { symbol(&library, "fmi3DeserializeFMUState") }?,
            })
        })();
        Ok(Self {
            get_version: req!("fmi3GetVersion"),
            instantiate: req!("fmi3InstantiateCoSimulation"),
            free_instance: req!("fmi3FreeInstance"),
            enter_initialization_mode: req!("fmi3EnterInitializationMode"),
            exit_initialization_mode: req!("fmi3ExitInitializationMode"),
            terminate: req!("fmi3Terminate"),
            reset: unsafe { symbol(&library, "fmi3Reset") },
            do_step: req!("fmi3DoStep"),
            float64: access!("fmi3GetFloat64", "fmi3SetFloat64"),
            float32: access!("fmi3GetFloat32", "fmi3SetFloat32"),
            int8: access!("fmi3GetInt8", "fmi3SetInt8"),
            uint8: access!("fmi3GetUInt8", "fmi3SetUInt8"),
            int16: access!("fmi3GetInt16", "fmi3SetInt16"),
            uint16: access!("fmi3GetUInt16", "fmi3SetUInt16"),
            int32: access!("fmi3GetInt32", "fmi3SetInt32"),
            uint32: access!("fmi3GetUInt32", "fmi3SetUInt32"),
            int64: access!("fmi3GetInt64", "fmi3SetInt64"),
            uint64: access!("fmi3GetUInt64", "fmi3SetUInt64"),
            boolean: access!("fmi3GetBoolean", "fmi3SetBoolean"),
            state,
            _library: library,
        })
    }

    /// The FMI version string the binary reports (`fmi3GetVersion`).
    pub fn version(&self) -> String {
        let p = unsafe { (self.get_version)() };
        if p.is_null() {
            return String::new();
        }
        unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
