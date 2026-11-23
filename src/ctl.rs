//! Remote control for scripts and tests: a tiny HTTP server on 127.0.0.1.
//!
//!   curl -s localhost:47811/cmd -d '{"cmd":"query","q":"#pixiv score>=4"}'
//!
//! Each request is a JSON object with "cmd"; the UI thread runs it in order
//! with the user's own clicks and answers with JSON. Only this PC can connect.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use eframe::egui;

pub const PORT: u16 = 47811;

pub struct Request {
    pub body: serde_json::Value,
    pub reply: Sender<serde_json::Value>,
}

pub fn start(ctx: egui::Context, tx: Sender<Request>) -> Option<u16> {
    let mut listener = None;
    let mut port = PORT;
    for p in PORT..PORT + 10 {
        if let Ok(l) = TcpListener::bind(("127.0.0.1", p)) {
            listener = Some(l);
            port = p;
            break;
        }
    }
    let listener = listener?;
    std::thread::Builder::new()
        .name("ctl".into())
        .spawn(move || {
            for s in listener.incoming().flatten() {
                let tx = tx.clone();
                let ctx = ctx.clone();
                std::thread::spawn(move || handle(s, tx, ctx));
            }
        })
        .ok()?;
    Some(port)
}

fn handle(mut s: TcpStream, tx: Sender<Request>, ctx: egui::Context) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(rs) = s.try_clone() else { return };
    let mut r = BufReader::new(rs);
    let mut first = String::new();
    if r.read_line(&mut first).is_err() {
        return;
    }
    let mut len = 0usize;
    let mut from_browser = false;
    let mut header_lines = 0;
    loop {
        header_lines += 1;
        if header_lines > 100 {
            return;
        }
        let mut line = String::new();
        if r.read_line(&mut line).is_err() || line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
            // a web page in a browser always sends Origin; scripts and curl don't
            if k.eq_ignore_ascii_case("origin") || k.eq_ignore_ascii_case("sec-fetch-site") {
                from_browser = true;
            }
            // DNS rebinding: only accept our own host names
            if k.eq_ignore_ascii_case("host") {
                let h = v.trim().split(':').next().unwrap_or("").to_ascii_lowercase();
                if h != "127.0.0.1" && h != "localhost" {
                    from_browser = true;
                }
            }
        }
    }
    let mut body = vec![0u8; len.min(1 << 20)];
    let _ = r.read_exact(&mut body);
    if from_browser {
        let _ = s.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    }
    let path = first.split_whitespace().nth(1).unwrap_or("/");
    let value: serde_json::Value = if path.starts_with("/state") {
        serde_json::json!({"cmd": "state"})
    } else {
        serde_json::from_slice(&body).unwrap_or_else(|e| serde_json::json!({"cmd": "_bad", "error": e.to_string()}))
    };
    let (rtx, rrx) = channel();
    let out = if tx.send(Request { body: value, reply: rtx }).is_ok() {
        ctx.request_repaint();
        rrx.recv_timeout(Duration::from_secs(20)).unwrap_or_else(|_| serde_json::json!({"ok": false, "error": "timeout"}))
    } else {
        serde_json::json!({"ok": false, "error": "app closing"})
    };
    let text = serde_json::to_string_pretty(&out).unwrap_or_default();
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        text.len(),
        text
    );
    let _ = s.write_all(resp.as_bytes());
}
