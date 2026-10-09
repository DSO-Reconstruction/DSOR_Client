//! Bug dumps: what a player sends when something goes wrong.
//!
//! F9 (or Ctrl+Shift+D) writes `dsor-dump-<time>.json`: in the browser it is
//! downloaded, natively it is written to the working directory. The same file is
//! written by itself when the client panics or when bevy quits on a rendering
//! error (a lost WebGL context, an invalid pipeline).
//!
//! It holds the build, the page URL or command line (the session id taken out),
//! the browser and its WebGL renderer, the state of the game once a second (map,
//! camera, player, network, frame times, the look settings, the debug flags) and
//! the last LOG_LINES log lines, warnings and errors included.
//!
//! The log lines come from a tracing layer added to bevy's LogPlugin
//! (`log_layer`), so everything bevy and wgpu log is in the dump, not only ours.

use std::collections::VecDeque;
use std::sync::Mutex;

use bevy::diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::log::tracing_subscriber::{layer::Context, Layer, Registry};
use bevy::log::{tracing, BoxedLayer};
use bevy::prelude::*;

/// How many log lines a dump keeps (the newest).
const LOG_LINES: usize = 600;

static LOGS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
/// The game state, refreshed once a second (JSON).
static STATE: Mutex<Option<serde_json::Value>> = Mutex::new(None);
/// Set once a dump has been written for a crash, so one crash writes one file.
static CRASHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys_now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    }
}

#[cfg(target_arch = "wasm32")]
fn js_sys_now() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.time_origin() + p.now()).unwrap_or(0.0)
}

fn push_line(line: String) {
    if let Ok(mut logs) = LOGS.lock() {
        if logs.len() >= LOG_LINES {
            logs.pop_front();
        }
        logs.push_back(line);
    }
}

/// Every log event, formatted on one line, into the dump's ring buffer.
struct DumpLayer;

struct Message(String);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0.insert_str(0, &format!("{value:?} "));
        } else {
            self.0.push_str(&format!("{}={value:?} ", field.name()));
        }
    }
}

impl Layer<Registry> for DumpLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, Registry>) {
        let meta = event.metadata();
        let mut message = Message(String::new());
        event.record(&mut message);
        let text = message.0.trim_end().to_owned();
        let quits = *meta.level() == tracing::Level::ERROR && text.contains("Quitting the application");
        push_line(format!("{:.0} {} {}: {}", now_ms(), meta.level(), meta.target(), text));
        if quits {
            crash_dump("render error: the application quits");
        }
    }
}

/// For LogPlugin::custom_layer.
pub fn log_layer(_: &mut App) -> Option<BoxedLayer> {
    Some(Box::new(DumpLayer))
}

/// Write a dump after a crash, once.
fn crash_dump(reason: &str) {
    if CRASHED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    write_dump(Some(reason));
}

/// Chain a panic hook that records the panic and writes a dump.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        push_line(format!("{:.0} PANIC {info}", now_ms()));
        crash_dump(&format!("panic: {info}"));
        previous(info);
    }));
}

/// The page URL or the command line, without the session id.
fn launch() -> String {
    #[cfg(target_arch = "wasm32")]
    let raw = web_sys::window().and_then(|w| w.location().href().ok()).unwrap_or_default();
    #[cfg(not(target_arch = "wasm32"))]
    let raw = std::env::args().collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    let mut hide_next = false;
    for part in raw.split_inclusive(|c| c == '&' || c == '?' || c == ' ') {
        if hide_next {
            out.push("<hidden> ".to_owned());
            hide_next = false;
            continue;
        }
        if part.starts_with("sid=") {
            out.push(format!("sid=<hidden>{}", if part.ends_with('&') { "&" } else { "" }));
        } else {
            hide_next = part.trim() == "--sid";
            out.push(part.to_owned());
        }
    }
    out.concat()
}

/// The browser and its WebGL renderer.
fn platform() -> serde_json::Value {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let window = web_sys::window();
        let agent = window.as_ref().and_then(|w| w.navigator().user_agent().ok()).unwrap_or_default();
        let renderer = window
            .as_ref()
            .and_then(|w| w.document())
            .and_then(|d| d.create_element("canvas").ok())
            .and_then(|c| c.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .and_then(|c| c.get_context("webgl2").ok().flatten())
            .and_then(|g| g.dyn_into::<web_sys::WebGl2RenderingContext>().ok())
            .map(|g| {
                // WEBGL_debug_renderer_info: UNMASKED_VENDOR 0x9245, UNMASKED_RENDERER 0x9246.
                let s = |p: u32| g.get_parameter(p).ok().and_then(|v| v.as_string()).unwrap_or_default();
                let _ = g.get_extension("WEBGL_debug_renderer_info");
                format!("{} / {} / {}", s(0x9245), s(0x9246), s(web_sys::WebGl2RenderingContext::VERSION))
            })
            .unwrap_or_else(|| "no webgl2".to_owned());
        serde_json::json!({ "user_agent": agent, "webgl": renderer })
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        serde_json::json!({ "os": std::env::consts::OS, "arch": std::env::consts::ARCH })
    }
}

fn write_dump(reason: Option<&str>) {
    let logs: Vec<String> = LOGS.lock().map(|l| l.iter().cloned().collect()).unwrap_or_default();
    let state = STATE.lock().ok().and_then(|s| s.clone()).unwrap_or(serde_json::Value::Null);
    let dump = serde_json::json!({
        "dump": 1,
        "reason": reason.unwrap_or("asked by the player"),
        "time_ms": now_ms(),
        "build": {
            "version": env!("CARGO_PKG_VERSION"),
            "commit": option_env!("DSOR_GIT_COMMIT").unwrap_or("unknown"),
            "target": if cfg!(target_arch = "wasm32") { "wasm32" } else { "native" },
            "graphics": if !cfg!(target_arch = "wasm32") { "native" } else if crate::WEBGL2 { "webgl2" } else { "webgpu" },
        },
        "launch": launch(),
        "platform": platform(),
        "state": state,
        "logs": logs,
    });
    let text = serde_json::to_string_pretty(&dump).unwrap_or_default();
    let name = format!("dsor-dump-{:.0}.json", now_ms());
    save(&name, &text);
}

#[cfg(target_arch = "wasm32")]
fn save(name: &str, text: &str) {
    use wasm_bindgen::JsCast;
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let parts = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(text));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/json");
    let Ok(blob) = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options) else { return };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else { return };
    if let Some(a) = document.create_element("a").ok().and_then(|a| a.dyn_into::<web_sys::HtmlAnchorElement>().ok()) {
        a.set_href(&url);
        a.set_download(name);
        a.click();
    }
    let _ = web_sys::Url::revoke_object_url(&url);
}

#[cfg(not(target_arch = "wasm32"))]
fn save(name: &str, text: &str) {
    match std::fs::write(name, text) {
        Ok(()) => eprintln!("dump written: {name}"),
        Err(e) => eprintln!("dump {name} not written: {e}"),
    }
}

/// The game state the dump carries, once a second.
#[allow(clippy::too_many_arguments)]
fn snapshot(
    time: Res<Time>,
    mut last: Local<f64>,
    current: Option<Res<crate::map::CurrentMap>>,
    asset_server: Res<AssetServer>,
    diagnostics: Res<DiagnosticsStore>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    players: Query<&GlobalTransform, With<crate::net::LocalPlayer>>,
    net: Option<NonSend<crate::net::Net>>,
    config: Option<Res<crate::net::NetConfig>>,
    lighting: Res<crate::lighting::Lighting>,
) {
    let t = time.elapsed_secs_f64();
    if t - *last < 1.0 {
        return;
    }
    *last = t;
    let v3 = |v: Vec3| [v.x, v.y, v.z];
    let diag = |d: &bevy::diagnostic::DiagnosticPath| diagnostics.get(d).and_then(|d| d.smoothed());
    let map = current.as_ref().map(|c| {
        let (done, total) = c.progress(&asset_server);
        serde_json::json!({ "name": c.name, "spawned": c.spawned, "models_loaded": done, "models": total, "instances": c.instances })
    });
    let camera = cameras.iter().next().map(|c| {
        let tf = c.compute_transform();
        serde_json::json!({ "position": v3(tf.translation), "forward": v3(tf.forward().as_vec3()) })
    });
    let net = net.as_ref().map(|n| {
        serde_json::json!({
            "local_actor": n.local_actor,
            "local_name": n.local_name,
            "health": n.health,
            "resource": n.resource,
            "server": config.as_ref().map(|c| c.login.clone()),
            "relay": config.as_ref().and_then(|c| c.relay.clone()),
        })
    });
    let state = serde_json::json!({
        "uptime_s": t,
        "map": map,
        "camera": camera,
        "player": players.iter().next().map(|p| v3(p.translation())),
        "net": net,
        "fps": diag(&FrameTimeDiagnosticsPlugin::FPS),
        "frame_ms": diag(&FrameTimeDiagnosticsPlugin::FRAME_TIME),
        "entities": diagnostics.get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT).and_then(|d| d.value()),
        "look": serde_json::to_value(&*lighting).ok(),
        "flags": crate::FLAGS.get().map(|f| f.iter().cloned().collect::<Vec<_>>()),
    });
    if let Ok(mut s) = STATE.lock() {
        *s = Some(state);
    }
}

fn dump_key(keys: Res<ButtonInput<KeyCode>>) {
    let ctrl_shift = (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight))
        && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight));
    if keys.just_pressed(KeyCode::F9) || (ctrl_shift && keys.just_pressed(KeyCode::KeyD)) {
        info!("bug dump requested");
        write_dump(None);
    }
}

pub struct DumpPlugin;

impl Plugin for DumpPlugin {
    fn build(&self, app: &mut App) {
        install_panic_hook();
        app.add_systems(Update, (snapshot, dump_key));
    }
}
