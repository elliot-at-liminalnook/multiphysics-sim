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
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|a| a == "--capability-socket") {
        if args.len() != 4 || args[2] != "--identity-file" {
            return Err("hx_virtual_bench --capability-socket SOCKET --identity-file JSON".into());
        }
        return serve_capability(&args[1], &args[3]);
    }
    if args.iter().any(|a| a.starts_with("--")) || args.len() > 1 {
        return Err("hx_virtual_bench [legacy-path-file] | --capability-socket SOCKET --identity-file JSON".into());
    }
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
    let port = unsafe { File::from_raw_fd(fd) };
    println!("{name}");
    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(path, &name)?;
    }
    run_port(port, &mut new_bench(), false)
}

fn new_bench() -> Bench {
    // Knee, worm, belt/hip: identified-like responses; the knee is sticky and
    // the belt/hip carries some gravity toward decreasing counts.
    let knee = MotorModel { breakaway_duty: 0.14, moving_friction_duty: 0.07, ..Default::default() };
    let worm = MotorModel { speed_gain: 3290., breakaway_duty: 0.066, moving_friction_duty: 0.045, ..Default::default() };
    let belt = MotorModel { speed_gain: 3030., breakaway_duty: 0.08, moving_friction_duty: 0.06, load_duty: -0.02, ..Default::default() };
    Bench::new([3100., 1100., 1500.], [knee, worm, belt])
}

fn run_port(mut port: File, bench: &mut Bench, socket: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut pending: Vec<u8> = Vec::new();
    let mut clock = Instant::now();
    loop {
        let mut buf = [0u8; 256];
        match port.read(&mut buf) {
            Ok(n) if n > 0 => pending.extend_from_slice(&buf[..n]),
            Ok(0) if socket => return Ok(()),
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
                if socket {
                    // A partial frame would desynchronize the peer's parser:
                    // finish it or end the session.
                    write_frame(&mut port, &reply)?;
                } else {
                    let _ = port.write_all(&reply);
                }
            }
        }
        std::thread::sleep(Duration::from_micros(300));
    }
}

/// Writes a whole reply on the non-blocking capability socket, retrying
/// `WouldBlock`/`Interrupted` until a short deadline; any other failure or the
/// deadline ends the session instead of leaving a partial frame.
fn write_frame(port: &mut File, mut frame: &[u8]) -> std::io::Result<()> {
    use std::io::ErrorKind;
    let deadline = Instant::now() + Duration::from_secs(1);
    while !frame.is_empty() {
        match port.write(frame) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => frame = &frame[n..],
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
                if Instant::now() >= deadline {
                    return Err(std::io::Error::new(ErrorKind::TimedOut, "virtual bench reply write timed out; session ended"));
                }
                std::thread::sleep(Duration::from_micros(200));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// A local capability endpoint whose only peer is the simulated HX bench.
/// The server consumes socket bytes directly; it never receives a serial path.
fn serve_capability(socket: &str, identity_file: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::{fd::OwnedFd, unix::{fs::PermissionsExt, net::UnixListener}};
    if !std::path::Path::new(socket).is_absolute() || !std::path::Path::new(identity_file).is_absolute() {
        return Err("Capability socket and identity file must be absolute new paths".into());
    }
    if std::path::Path::new(socket).exists() || std::path::Path::new(identity_file).exists() {
        return Err("Capability paths already exist; retained evidence is never overwritten".into());
    }
    let listener = UnixListener::bind(socket)?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
    let bench_instance = sim_runtime::hardware_client::new_client_id();
    let handshake = serde_json::json!({"schema_version":1,"kind":"virtual_calibration","bench_instance":bench_instance});
    let mut record = std::fs::OpenOptions::new().write(true).create_new(true).open(identity_file)?;
    serde_json::to_writer_pretty(&mut record, &serde_json::json!({"schema_version":1,"kind":"virtual_calibration","bench_instance":bench_instance,"capability_socket":socket}))?;
    record.flush()?;
    println!("Virtual calibration capability: {socket}; bench {bench_instance}");
    let mut bench = new_bench();
    for connection in listener.incoming() {
        // Per-connection failures end that connection only; the listener
        // (and the bench state behind it) keeps serving.
        let mut stream = match connection {
            Ok(stream) => stream,
            Err(error) => { eprintln!("Virtual connection refused: {error}"); continue; }
        };
        if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(2))).and_then(|_| stream.set_write_timeout(Some(Duration::from_secs(2)))) {
            eprintln!("Virtual connection setup failed: {error}");
            continue;
        }
        let mut greeting = [0u8; b"HX-VIRTUAL-CALIBRATION/1\n".len()];
        let deadline = Instant::now() + Duration::from_secs(2);
        let greeted = greeting.iter_mut().all(|byte| {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else { return false; };
            stream.set_read_timeout(Some(remaining.max(Duration::from_millis(1)))).is_ok()
                && stream.read_exact(std::slice::from_mut(byte)).is_ok()
        });
        if !greeted || &greeting != b"HX-VIRTUAL-CALIBRATION/1\n" { continue; }
        let ready = writeln!(stream, "{handshake}")
            .and_then(|_| stream.set_read_timeout(None))
            .and_then(|_| stream.set_write_timeout(None))
            .and_then(|_| stream.set_nonblocking(true));
        if let Err(error) = ready {
            eprintln!("Virtual handshake failed: {error}");
            continue;
        }
        let descriptor: OwnedFd = stream.into();
        let result = run_port(File::from(descriptor), &mut bench, true);
        // Descriptor loss interrupts all axes before a subsequent connection.
        let _ = bench.handle(254, 0xa0, &[0]);
        if let Err(error) = result { eprintln!("Virtual session ended: {error}"); }
    }
    Ok(())
}
