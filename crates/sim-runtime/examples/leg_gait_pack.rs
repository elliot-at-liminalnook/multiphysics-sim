//! Build the robot-link ESP32's gait pack for the printed leg, and optionally
//! upload it.
//!
//!     cargo run -p sim-runtime --example leg_gait_pack -- \
//!         examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json \
//!         --out /tmp/leg.rlgp [--effort 0.5] [--hz 25] [--supply 11.1] \
//!         [--gait examples/.../compiled.json]... [--max 12] [--bindings RUN.json] \
//!         [--svg /tmp/leg.svg] [--push 192.168.1.39]
//!
//!     ... leg_gait_pack -- SERVER.json --pull 192.168.1.39
//!
//!     ... leg_gait_pack -- SERVER.json --pull-calibration 192.168.1.39 [--reason "..."]
//!
//! `--pull` saves the ESP32's last gait run (`GET /api/gait_log`) into the
//! leg's `gait-runs/` folder as `esp32-run-<ms>.json`, with tracking
//! statistics, beside the panel's runs.
//!
//! `--pull-calibration` promotes poses taught on the ESP32's page into the
//! leg's `calibration.json` (`gait_pack::promote_esp32_poses`); a widened
//! travel needs `--reason`. Then build and push the pack again. `--push`
//! refuses to replace poses taught on the ESP32 that the new pack does not
//! carry, unless `--discard-esp32-poses`.
//!
//! The pack is built by `sim_runtime::hardware::calibration::gait_pack` from
//! the same sources the calibration panel plays from. `--svg` draws every
//! gait's first frame and the live-pose grid's corners, to check the drawing.
use sim_runtime::hardware::calibration::gait_pack::{PackOptions, build_gait_pack, esp32_run_record, promote_esp32_poses, read_pack};
use std::io::{Read, Write};

fn main() {
    if let Err(e) = run() {
        eprintln!("leg_gait_pack: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let config = args.next().ok_or("usage: leg_gait_pack SERVER.json --out FILE [options]")?;
    let mut options = PackOptions::default();
    let (mut out, mut svg, mut push, mut pull) = (None, None, None, None);
    let (mut pull_calibration, mut reason, mut discard, mut live) = (None, None, false, None);
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--out" => out = Some(value()?),
            "--effort" => options.effort = value()?.parse().map_err(|e| format!("--effort: {e}"))?,
            "--hz" => options.sample_hz = value()?.parse().map_err(|e| format!("--hz: {e}"))?,
            "--supply" => options.supply_v = value()?.parse().map_err(|e| format!("--supply: {e}"))?,
            "--gait" => options.gaits.push(value()?),
            "--max" => options.max_gaits = value()?.parse().map_err(|e| format!("--max: {e}"))?,
            "--bindings" => options.bindings_from = Some(value()?.into()),
            "--svg" => svg = Some(value()?),
            "--push" => push = Some(value()?),
            "--pull" => pull = Some(value()?),
            "--pull-calibration" => pull_calibration = Some(value()?),
            "--reason" => reason = Some(value()?),
            "--discard-esp32-poses" => discard = true,
            "--live" => live = Some(value()?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if let Some(host) = pull {
        return pull_run(&config, &host);
    }
    if let Some(host) = pull_calibration {
        let esp32: serde_json::Value = serde_json::from_str(&http(&host, "GET", "/api/calibration", &[])?).map_err(|e| format!("calibration: {e}"))?;
        let out = promote_esp32_poses(std::path::Path::new(&config), &esp32, reason.as_deref())?;
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        println!("now build and push the pack: leg_gait_pack {config} --push {host}");
        return Ok(());
    }
    let pack = build_gait_pack(std::path::Path::new(&config), &options)?;
    let bytes = pack.to_bytes();
    read_pack(&bytes)?;
    let gaits = pack.json["gaits"].as_array().cloned().unwrap_or_default();
    let playable = gaits.iter().filter(|g| g["playable"] == true).count();
    println!("{} gaits ({} playable on the leg), {} motors commandable, {} bytes", gaits.len(), playable, pack.json["axes"].as_array().map_or(0, Vec::len), bytes.len());
    for e in pack.json["excluded"].as_array().into_iter().flatten() {
        println!("  excluded {}: {}", e["role"].as_str().unwrap_or("?"), e["reason"].as_str().unwrap_or("?"));
    }
    for g in &gaits {
        if g["playable"] != true {
            println!("  preview only {}: {}", g["name"].as_str().unwrap_or("?"), g["reasons"]);
        }
    }
    if let Some(path) = out {
        std::fs::write(&path, &bytes).map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}");
    }
    if let Some(path) = svg {
        let state = match &live {
            Some(host) => Some(serde_json::from_str::<serde_json::Value>(&http(host, "GET", "/api/state", &[])?).map_err(|e| format!("state: {e}"))?),
            None => None,
        };
        std::fs::write(&path, preview_svg(&pack.json, state.as_ref())?).map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}");
    }
    if let Some(host) = push {
        // Storing a pack replaces travel taught on the ESP32's page only for a motor whose travel
        // the pack changes; refuse that unless the pack carries what was taught.
        let esp32: serde_json::Value = serde_json::from_str(&http(&host, "GET", "/api/calibration", &[])?).unwrap_or_default();
        let lost: Vec<String> = esp32["motors"].as_array().into_iter().flatten().filter(|m| m["changed"] == true).filter(|m| {
            let axis = pack.json["axes"].as_array().and_then(|a| a.iter().find(|x| x["id"] == m["id"]));
            axis.is_none_or(|a| a["travel_counts"] != m["pack"] && (a["travel_counts"] != m["taught"] || m["open"].as_array().is_some_and(|o| o.iter().any(|x| x == true))))
        }).map(|m| format!("motor {} ({})", m["id"], m["taught"])).collect();
        if !lost.is_empty() && !discard {
            return Err(format!("the ESP32 has poses taught on its page that this pack does not carry: {}. Promote them first (--pull-calibration {host}), or pass --discard-esp32-poses", lost.join(", ")));
        }
        let reply = post(&host, "/api/gaits", &bytes)?;
        println!("pushed to {host}: {reply}");
    }
    Ok(())
}

/// The leg figure composed from the pack exactly as the page composes it:
/// each link's transform relative to its parent, read from its motor's table
/// (linear between samples), multiplied down the chain.
struct Figure<'a> {
    f: &'a serde_json::Value,
}
type Rt = [f64; 12];
impl Figure<'_> {
    fn rt(v: &serde_json::Value) -> Rt {
        std::array::from_fn(|k| v[k].as_f64().unwrap_or(0.))
    }
    fn mul(a: &Rt, b: &Rt) -> Rt {
        let mut o = [0.; 12];
        for i in 0..3 {
            for j in 0..3 {
                o[4 * i + j] = (0..3).map(|k| a[4 * i + k] * b[4 * k + j]).sum();
            }
            o[4 * i + 3] = (0..3).map(|k| a[4 * i + k] * b[4 * k + 3]).sum::<f64>() + a[4 * i + 3];
        }
        o
    }
    fn compose(&self, angles: &std::collections::HashMap<u64, f64>) -> Vec<Rt> {
        let links = self.f["links"].as_array().unwrap();
        let motors = self.f["motors"].as_array().unwrap();
        let mut world = vec![[0.; 12]; links.len()];
        for k in self.f["order"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize) {
            let l = &links[k];
            let rel = match l["motor"].as_u64() {
                None => Self::rt(&l["rel"]),
                Some(id) => {
                    let m = motors.iter().find(|m| m["id"] == id).unwrap();
                    let (lo, hi, n) = (m["rad"][0].as_f64().unwrap(), m["rad"][1].as_f64().unwrap(), m["samples"].as_u64().unwrap() as usize);
                    let x = ((angles.get(&id).copied().unwrap_or((lo + hi) / 2.) - lo) / (hi - lo)).clamp(0., 1.) * (n - 1) as f64;
                    let i = (x.floor() as usize).min(n - 2);
                    let (a, b) = (Self::rt(&l["table"][i]), Self::rt(&l["table"][i + 1]));
                    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * (x - i as f64))
                }
            };
            world[k] = match l["parent"].as_u64() {
                Some(p) => Self::mul(&world[p as usize], &rel),
                None => rel,
            };
        }
        world
    }
}
fn link_colour(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.contains("crosshead") || n.contains("foot rod") { "#3fa96b" }
    else if n.contains("crank") || n.contains("connecting") { "#e0913a" }
    else if n.contains("thigh") { "#4a86d8" }
    else if n.contains("worm") { "#b07cd6" }
    else { "#8a93a0" }
}
/// One view of the figure at `world`: every link's triangles filled, far links first.
fn draw_view(out: &mut String, f: &serde_json::Value, world: &[Rt], view: &serde_json::Value, x0: f64, y0: f64, size: f64, label: &str) {
    let g = |v: &serde_json::Value| -> [f64; 3] { std::array::from_fn(|k| v[k].as_f64().unwrap()) };
    let (u, v) = (g(&view["u"]), g(&view["v"]));
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let b: Vec<f64> = view["bounds"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let scale = 0.9 * size / (b[1] - b[0]).max(b[3] - b[2]);
    let (cu, cv) = ((b[0] + b[1]) / 2., (b[2] + b[3]) / 2.);
    let px = |p: [f64; 3]| -> (f64, f64) {
        let (a, c) = (p[0] * u[0] + p[1] * u[1] + p[2] * u[2], p[0] * v[0] + p[1] * v[1] + p[2] * v[2]);
        (x0 + size / 2. + (a - cu) * scale, y0 + size / 2. - (c - cv) * scale)
    };
    *out += &format!(r##"<rect x="{x0}" y="{y0}" width="{size}" height="{size}" fill="#16181d"/><text x="{}" y="{}" fill="#9aa0a6" font-size="12" font-family="sans-serif">{label}</text>"##, x0 + 6., y0 + 16.);
    let links = f["links"].as_array().unwrap();
    let mut placed: Vec<(f64, usize, Vec<[f64; 3]>)> = links.iter().enumerate().map(|(k, l)| {
        let vs: Vec<f64> = l["vertices"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        let w: Vec<[f64; 3]> = vs.chunks(3).map(|c| { let t = &world[k]; std::array::from_fn(|i| t[4 * i] * c[0] + t[4 * i + 1] * c[1] + t[4 * i + 2] * c[2] + t[4 * i + 3]) }).collect();
        let depth = w.iter().map(|p| p[0] * n[0] + p[1] * n[1] + p[2] * n[2]).sum::<f64>() / w.len().max(1) as f64;
        (depth, k, w)
    }).collect();
    placed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (_, k, w) in &placed {
        let colour = link_colour(links[*k]["name"].as_str().unwrap_or(""));
        let tris: Vec<usize> = links[*k]["triangles"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect();
        *out += &format!(r##"<g fill="{colour}" stroke="{colour}" stroke-width="0.4">"##);
        for t in tris.chunks(3) {
            let (a, b, c) = (px(w[t[0]]), px(w[t[1]]), px(w[t[2]]));
            *out += &format!(r##"<path d="M{:.1} {:.1}L{:.1} {:.1}L{:.1} {:.1}Z"/>"##, a.0, a.1, b.0, b.1, c.0, c.1);
        }
        *out += "</g>";
    }
}
/// A sheet of the figure, side and top, at CAD home, four phases of the first
/// gait and, with the ESP32's state, the leg as read now (each place a
/// multi-turn motor's reading allows when its turn is not confirmed).
fn preview_svg(pack: &serde_json::Value, state: Option<&serde_json::Value>) -> Result<String, String> {
    let f = &pack["figure"];
    let fig = Figure { f };
    let motors: Vec<u64> = f["motors"].as_array().ok_or("pack has no figure")?.iter().map(|m| m["id"].as_u64().unwrap()).collect();
    let mut rows: Vec<(String, std::collections::HashMap<u64, f64>)> = Vec::new();
    let axis = |id: u64| pack["axes"].as_array().unwrap().iter().find(|a| a["id"] == id).unwrap().clone();
    let rad = |id: u64, counts: f64| { let a = axis(id); a["home_rad"].as_f64().unwrap() + a["polarity"].as_f64().unwrap() * (counts - a["reference_counts"].as_f64().unwrap()) * std::f64::consts::TAU / 4096. };
    rows.push(("alignment pose (each motor at its saved reference)".into(), motors.iter().map(|&id| (id, axis(id)["home_rad"].as_f64().unwrap())).collect()));
    if let Some(g) = pack["gaits"].get(0) {
        let frames = g["frames_rad"].as_array().unwrap();
        for q in [0., 0.25, 0.5, 0.75] {
            let fr = &frames[(q * frames.len() as f64) as usize];
            rows.push((format!("{} at {:.0}% of its cycle", g["name"].as_str().unwrap_or("gait"), q * 100.), motors.iter().enumerate().map(|(i, &id)| (id, fr[i].as_f64().unwrap())).collect()));
        }
    }
    if let Some(s) = state {
        let servos = s["servos"].as_array().ok_or("state has no servos")?;
        let reading = |id: u64| servos.iter().find(|v| v["id"] == id);
        let mut base = std::collections::HashMap::new();
        let mut places: Vec<(u64, f64)> = Vec::new();
        for &id in &motors {
            let Some(v) = reading(id) else { continue };
            if axis(id)["multi_turn"] == true && v["turn_known"] != true {
                for c in v["travel"]["candidates"].as_array().into_iter().flatten() {
                    places.push((id, c.as_f64().unwrap()));
                }
            } else if let Some(p) = v["position"].as_f64() {
                base.insert(id, rad(id, p));
            }
        }
        if places.is_empty() {
            rows.push(("the leg as read now".into(), base.clone()));
        }
        for (id, c) in places {
            let mut q = base.clone();
            q.insert(id, rad(id, c));
            rows.push((format!("as read now, {} at {c:.0} counts", axis(id)["role"].as_str().unwrap_or("motor")), q));
        }
    }
    let size = 300.;
    let views = f["views"].as_array().unwrap();
    let width = 20. + (size + 20.) * views.len() as f64;
    let mut out = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{}" style="background:#101216">"##, 20. + (size + 40.) * rows.len() as f64);
    for (r, (label, angles)) in rows.iter().enumerate() {
        let world = fig.compose(angles);
        let y = 20. + r as f64 * (size + 40.);
        out += &format!(r##"<text x="20" y="{}" fill="#e6e6e6" font-size="14" font-family="sans-serif">{label}</text>"##, y + 12.);
        for (c, v) in views.iter().enumerate() {
            draw_view(&mut out, f, &world, v, 20. + c as f64 * (size + 20.), y + 20., size, v["name"].as_str().unwrap_or(""));
        }
    }
    Ok(out + "</svg>")
}

fn pull_run(config: &str, host: &str) -> Result<(), String> {
    let log: serde_json::Value = serde_json::from_str(&http(host, "GET", "/api/gait_log", &[])?).map_err(|e| format!("gait log: {e}"))?;
    let pack: serde_json::Value = serde_json::from_str(&http(host, "GET", "/api/gaits", &[])?).map_err(|e| format!("pack: {e}"))?;
    let state: serde_json::Value = serde_json::from_str(&http(host, "GET", "/api/state", &[])?).map_err(|e| format!("state: {e}"))?;
    let record = esp32_run_record(&log, &pack, state["pack"]["sha256"].as_str().unwrap_or("unknown pack"))?;
    let cfg: serde_json::Value = serde_json::from_slice(&std::fs::read(config).map_err(|e| format!("{config}: {e}"))?).map_err(|e| format!("{config}: {e}"))?;
    let dir = std::path::Path::new(cfg["output"].as_str().ok_or("config has no output")?).join("gait-runs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
    let path = dir.join(format!("esp32-run-{stamp}.json"));
    std::fs::write(&path, serde_json::to_vec(&record).unwrap()).map_err(|e| e.to_string())?;
    println!("{}: {} — {}", path.display(), record["gait"], record["outcome"]);
    println!("statistics: {}", record["statistics"]);
    Ok(())
}

fn post(host: &str, path: &str, body: &[u8]) -> Result<String, String> {
    http(host, "POST", path, body)
}

/// A plain HTTP/1.1 request, so the tool needs no HTTP client crate.
fn http(host: &str, method: &str, path: &str, body: &[u8]) -> Result<String, String> {
    let addr = if host.contains(':') { host.to_string() } else { format!("{host}:80") };
    let mut stream = std::net::TcpStream::connect(&addr).map_err(|e| format!("{addr}: {e}"))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(90))).ok();
    let head = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
    for chunk in body.chunks(8192) {
        stream.write_all(chunk).map_err(|e| e.to_string())?;
    }
    // The ESP32's server keeps the connection open after its reply, whatever
    // the request says, so read until the reply is complete, not until EOF.
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    while !reply_complete(&raw) {
        let n = stream.read(&mut buf).map_err(|e| format!("{method} {path}: {e}"))?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
    }
    let reply = String::from_utf8_lossy(&raw).to_string();
    let status = reply.lines().next().unwrap_or("").to_string();
    let (head, rest) = reply.split_once("\r\n\r\n").unwrap_or((&reply, ""));
    // The ESP32 streams large replies chunked.
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") { dechunk(rest) } else { rest.trim().to_string() };
    if !status.contains(" 200 ") {
        return Err(format!("{status}: {body}"));
    }
    Ok(body)
}

/// Whether `raw` holds a whole HTTP reply: its head and a body of
/// Content-Length bytes, or a chunked body through its last chunk.
fn reply_complete(raw: &[u8]) -> bool {
    let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else { return false };
    let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
    let body = &raw[end + 4..];
    if head.contains("transfer-encoding: chunked") {
        return body.ends_with(b"0\r\n\r\n");
    }
    match head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()) {
        Some(n) => body.len() >= n,
        None => false,
    }
}

fn dechunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = s.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 || rest.len() < n {
            break;
        }
        out.push_str(&rest[..n]);
        s = rest[n..].trim_start_matches("\r\n");
    }
    out
}
