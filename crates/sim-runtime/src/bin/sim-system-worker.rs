#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(error) = sim_runtime::system_worker::serve(sim_runtime::registry()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    panic!("sim-system-worker requires native process support");
}
