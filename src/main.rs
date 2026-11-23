#![windows_subsystem = "windows"]

#[macro_use]
mod i18n;
mod app;
mod ctl;
mod db;
mod ops;
mod query;
mod settings;
mod store;
mod thumbs;
mod winshell;

/// "v0.23" from the Cargo version 0.23.0 (shown in the window title).
pub fn version_short() -> String {
    let v: Vec<&str> = env!("CARGO_PKG_VERSION").split('.').collect();
    format!("v{}.{}", v[0], v.get(1).unwrap_or(&"0"))
}

fn main() -> eframe::Result<()> {
    std::panic::set_hook(Box::new(|info| {
        let dir = db::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let msg = format!("{} {info}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")) {
            use std::io::Write;
            let _ = f.write_all(msg.as_bytes());
        }
    }));
    let offscreen = std::env::var_os("IMGR_OFFSCREEN").is_some();
    // one imgr per root (test runs too: they use their own roots)
    if already_running() {
        return Ok(());
    }
    let s = settings::Settings::load();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(format!("imgr {}", version_short()))
        .with_inner_size([s.win_w.max(640.0), s.win_h.max(420.0)])
        .with_min_inner_size([640.0, 420.0]);
    if let (Some(x), Some(y), false) = (s.win_x, s.win_y, offscreen) {
        // only if that spot is still on a connected monitor
        if winshell::point_on_screen(x + 60.0, y + 15.0) {
            viewport = viewport.with_position([x, y]);
        }
    }
    if offscreen {
        // test run (IMGR_OFFSCREEN): off-screen, not activated, no taskbar button
        viewport = viewport.with_position([-6000.0, -6000.0]).with_active(false).with_taskbar(false);
    }
    // Ctrl+C/X/V come to imgr as plain keys: egui would drop a Ctrl+V when the
    // clipboard holds files but no text. Text boxes get Copy/Cut/Paste from
    // App::raw_input_hook instead.
    let options = eframe::NativeOptions { viewport, clipboard_shortcuts: false, ..Default::default() };
    eframe::run_native("imgr", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

/// Another imgr on the same root? A named mutex decides (works even when the
/// other window is minimized); then we ask it to come to the front.
fn already_running() -> bool {
    let root = db::root_dir().display().to_string().to_lowercase();
    if !winshell::claim_instance(&root) {
        if !winshell::request_focus(&root) {
            focus_old(&root);
        }
        return true;
    }
    false
}

/// imgr up to v0.11 (no focus event): ask over its control port.
fn focus_old(root: &str) {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;
    for port in ctl::PORT..ctl::PORT + 10 {
        let Ok(addr) = format!("127.0.0.1:{port}").parse() else { continue };
        let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(150)) else { continue };
        let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
        if s.write_all(b"GET /state HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").is_err() {
            continue;
        }
        let mut buf = String::new();
        let _ = s.read_to_string(&mut buf);
        let body = buf.split("\r\n\r\n").nth(1).unwrap_or("");
        let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else { continue };
        if v.get("root").and_then(|r| r.as_str()).map(|r| r.to_lowercase()).as_deref() == Some(root) {
            let msg = r#"{"cmd":"focus"}"#;
            if let Ok(mut s2) = TcpStream::connect_timeout(&addr, Duration::from_millis(150)) {
                let _ = s2.write_all(format!("POST /cmd HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{}", msg.len(), msg).as_bytes());
                let _ = s2.set_read_timeout(Some(Duration::from_secs(3)));
                let mut sink = String::new();
                let _ = s2.read_to_string(&mut sink);
            }
            return;
        }
    }
}
