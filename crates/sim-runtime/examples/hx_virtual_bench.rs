//! Serves the simulated calibration bench (`acquisition::virtual_bench`) on a
//! pseudo-terminal, so `serve_actuator_calibration` and the browser panel run
//! end to end with no hardware. Prints the serial path to use in server.json.
//!
//!     cargo run --release -p sim-runtime --example hx_virtual_bench -- [path-file]
use sim_runtime::acquisition::virtual_bench::{Bench, MotorModel};
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::FromRawFd,
    time::{Duration, Instant},
};

unsafe extern "C" {
    fn posix_openpt(flags: i32) -> i32;
    fn grantpt(fd: i32) -> i32;
    fn unlockpt(fd: i32) -> i32;
    fn ptsname(fd: i32) -> *const std::ffi::c_char;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const O_RDWR: i32 = 2;
    const O_NOCTTY: i32 = if cfg!(target_os = "macos") { 0x20000 } else { 0o400 };
    const F_SETFL: i32 = 4;
    const O_NONBLOCK: i32 = if cfg!(target_os = "macos") { 4 } else { 2048 };
    let fd = unsafe { posix_openpt(O_RDWR | O_NOCTTY) };
    if fd < 0 || unsafe { grantpt(fd) } != 0 || unsafe { unlockpt(fd) } != 0 {
        return Err("could not open a pseudo-terminal".into());
    }
    let name = unsafe { std::ffi::CStr::from_ptr(ptsname(fd)) }.to_str()?.to_string();
    unsafe { fcntl(fd, F_SETFL, O_NONBLOCK) };
    let mut port = unsafe { File::from_raw_fd(fd) };
    println!("{name}");
    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(path, &name)?;
    }
    // Knee, worm, belt/hip: identified-like responses; the knee is sticky and
    // the belt/hip carries some gravity toward decreasing counts.
    let knee = MotorModel { breakaway_duty: 0.14, moving_friction_duty: 0.07, ..Default::default() };
    let worm = MotorModel { speed_gain: 3290., breakaway_duty: 0.066, moving_friction_duty: 0.045, ..Default::default() };
    let belt = MotorModel { speed_gain: 3030., breakaway_duty: 0.08, moving_friction_duty: 0.06, load_duty: -0.02, ..Default::default() };
    let mut bench = Bench::new([3100., 1100., 1500.], [knee, worm, belt]);
    let mut pending: Vec<u8> = Vec::new();
    let mut clock = Instant::now();
    loop {
        let mut buf = [0u8; 256];
        match port.read(&mut buf) {
            Ok(n) if n > 0 => pending.extend_from_slice(&buf[..n]),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) if e.raw_os_error() == Some(5) => {} // EIO: no client attached yet
            Err(e) => return Err(e.into()),
        }
        // Advance physics in real time.
        let dt = clock.elapsed().as_secs_f64();
        if dt >= 0.001 {
            bench.advance(dt);
            clock = Instant::now();
        }
        // Parse complete packets: FF FF id len inst params... checksum.
        loop {
            while pending.len() >= 2 && !(pending[0] == 0xff && pending[1] == 0xff) {
                pending.remove(0);
            }
            if pending.len() < 4 {
                break;
            }
            let total = pending[3] as usize + 4;
            if pending.len() < total {
                break;
            }
            let frame: Vec<u8> = pending.drain(..total).collect();
            let sum = frame[2..total - 1].iter().fold(0u8, |a, b| a.wrapping_add(*b));
            if !sum != frame[total - 1] {
                continue;
            }
            let reply = bench.handle(frame[2], frame[4], &frame[5..total - 1]);
            std::thread::sleep(Duration::from_secs_f64(bench.transaction_s));
            if !reply.is_empty() {
                let _ = port.write_all(&reply);
            }
        }
        std::thread::sleep(Duration::from_micros(300));
    }
}
