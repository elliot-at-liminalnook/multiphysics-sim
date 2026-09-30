//! The viewer's one REST server: binding, the window-visibility check the
//! `screenshot` command shares, the wake-up on a request and the headless
//! loop. Commands are typed actions (`app::actions`): the capabilities are
//! generated from the action registry, and one poll (`actions::serve`) hands
//! each command to its mode's action handler.
use super::*;
use serde_json::json;
/// The window's one REST server (every mode) and the scene `render` in flight.
#[derive(Resource)]
pub struct Rest(pub sim_api::Server, pub Option<sim_api::ImageTask>);

/// The one server of the viewer, windowed or `--headless`: every mode's
/// commands, each tagged with the modes it applies to, generated from the
/// action registry (`app::actions::capabilities`).
/// The only `sim_api::Server::bind` in sim-spatial.
pub fn bind(port: u16) -> std::io::Result<sim_api::Server> {
    let server = sim_api::Server::bind(port, "sim-spatial", crate::app::actions::capabilities())?;
    server.describe("workspace", crate::workspace::json());
    server.describe("modes", json!({
        "all": crate::ViewerMode::ALL,
        "rule": "each command lists the modes it applies to in `modes`; a command of another mode is refused naming the active mode. GET /v1/viewer_mode (or the command viewer_mode with {}) reports the active mode; viewer_mode {mode, path?, preset?} switches it. The headless server (--headless) serves inspect mode only.",
    }));
    Ok(server)
}

/// Whether the primary window is hidden (screen locked, minimized, fully
/// covered or on another Space). Since Bevy 0.19 (wgpu 29, gfx-rs/wgpu#8309)
/// macOS draws nothing into a hidden window, so a screenshot then would be
/// blank; `screenshot` refuses instead. On macOS this reads the same
/// `NSWindow.occlusionState` wgpu checks (winit sends no event for a window
/// created hidden); elsewhere it follows winit's occlusion events.
#[derive(Resource, Default)]
pub struct Occlusion(pub bool);

#[cfg(target_os = "macos")]
pub fn track_occlusion(window: Query<&bevy::window::RawHandleWrapper, With<bevy::window::PrimaryWindow>>, _main_thread: bevy::ecs::system::NonSendMarker, mut occlusion: ResMut<Occlusion>) {
    use objc2::{msg_send, runtime::AnyObject};
    let Ok(handle) = window.single() else { return };
    let raw_window_handle::RawWindowHandle::AppKit(appkit) = handle.get_window_handle() else { return };
    // NSWindowOcclusionStateVisible
    const VISIBLE: usize = 1 << 1;
    // SAFETY: the handle's NSView outlives the window entity (RawHandleWrapper
    // holds the window), and NonSendMarker keeps this on the main thread.
    let hidden = unsafe {
        let view = appkit.ns_view.as_ptr() as *const AnyObject;
        let ns_window: *const AnyObject = msg_send![&*view, window];
        !ns_window.is_null() && {
            let state: usize = msg_send![&*ns_window, occlusionState];
            state & VISIBLE == 0
        }
    };
    if occlusion.0 != hidden {
        occlusion.0 = hidden;
    }
}

#[cfg(not(target_os = "macos"))]
pub fn track_occlusion(mut events: MessageReader<bevy::window::WindowOccluded>, primary: Query<(), With<bevy::window::PrimaryWindow>>, mut occlusion: ResMut<Occlusion>) {
    for e in events.read() {
        if primary.contains(e.window) && occlusion.0 != e.occluded {
            occlusion.0 = e.occluded;
        }
    }
}

/// The `screenshot` command's shared validation: a `.png` path, and a visible window.
pub fn screenshot_path(args: &serde_json::Value, occluded: bool) -> Result<std::path::PathBuf, String> {
    let path = args.get("path").and_then(|p| p.as_str()).map(std::path::PathBuf::from).filter(|p| p.extension().is_some_and(|e| e == "png"));
    let Some(path) = path else { return Err("screenshot needs {\"path\": \"…/file.png\"}".into()) };
    if occluded {
        return Err("screenshot refused: the window is not visible (screen locked, minimized, fully covered or on another Space); macOS draws nothing into a hidden window (Bevy 0.19 / wgpu 29), so the PNG would be blank. Show the window and retry.".into());
    }
    Ok(path)
}

/// macOS naps background apps: timers and wake-ups are delayed by seconds.
/// A viewer with a REST server must answer promptly, so it declares a
/// user-initiated, latency-critical activity for as long as it runs (the
/// system may still sleep when idle).
fn keep_responsive() {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        let info = NSProcessInfo::processInfo();
        let reason = NSString::from_str("Answering REST commands");
        let token = info.beginActivityWithOptions_reason(NSActivityOptions::UserInitiatedAllowingIdleSystemSleep | NSActivityOptions::LatencyCritical, &reason);
        // Held for the life of the process.
        std::mem::forget(token);
    }
}

/// Wake the event loop when a REST command arrives, so a background window
/// answers promptly without drawing continuously.
pub(super) fn wake_on_request(rest: Option<Res<Rest>>, proxy: Option<Res<bevy::winit::EventLoopProxyWrapper>>) {
    let (Some(rest), Some(proxy)) = (rest, proxy) else { return };
    keep_responsive();
    let proxy = std::sync::Mutex::new((**proxy).clone());
    rest.0.set_waker(move || {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(bevy::winit::WinitUserEvent::WakeUp);
        }
    });
}

/// Runs the identical command adapter without creating a window or GPU context.
pub fn headless(
    mut scene: SpatialScene,
    mut link: Option<SelectionLink>,
    mut server: sim_api::Server,
) -> ! {
    let (focus, radius) = scene.bounds();
    let mut camera = Orbit {
        focus,
        radius: radius * 2.9,
        yaw: 0.7,
        pitch: 0.4,
        home: false,
        ..Default::default()
    };
    let mut image_task = None;
    loop {
        if let Some(link) = &mut link {
            if let Ok(target) = link.0.exchange(scene.selection.clone()) {
                if target != scene.selection {
                    let _ = scene.set_selection(target);
                }
            }
        }
        scene.poll_live();
        notes::sync(&mut scene, &mut camera);
        if camera.home {
            let (focus, radius) = scene.bounds();
            camera.focus = focus;
            camera.radius = radius * 2.9;
            camera.home = false;
        }
        crate::inspect::serve_headless(&mut server, &mut scene, &mut camera, &mut image_task);
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
