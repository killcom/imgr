//! ctl commands (JSON over localhost, see ctl.rs). Runs on the UI thread after
//! the frame is drawn, so "layout" rects are those of the frame just shown.

use std::path::PathBuf;

use eframe::egui::{self, Key, Modifiers, Pos2, vec2};
use serde_json::{Value, json};

use super::*;

pub(super) const CMDS: &str = "state query{q} list{limit} select{ids|index|all} score{n} tag{add,remove} trash{on} undo \
view{mode:grid|viewer|compare|matrix|albums,zoom,center} key{key,ctrl,shift} press{key,ctrl,shift,alt} type{text} \
mouse{op:move|click|dblclick|right|down|up|drag,x,y,to:[x,y],steps,ctrl,shift} wheel{x,y,dy,ctrl} \
thumb{size} window{w,h} layout screenshot{path} rescan import{from,move} save_filter{name,q} notice{text} \
albums album_new{name,parent} album_rename{id,name} album_delete{id} album_sel{ids,only} album_put{id|none} album_open{id|none|top} drop{files} rename{name|pattern,start|upload} export{dir,fmt,quality,upload,tag} panel{show_left,show_right} \
cmp_add{clear} cmp_row{row,op:remove|up|down|top|bottom} modal{close} focus save quit";

fn img_json(i: &Img) -> Value {
    json!({"id": i.id, "path": i.path, "score": i.score, "tags": i.tags, "preset": i.preset, "ckpt": i.ckpt,
           "seed": i.seed, "trash": i.trash, "albums": i.albums, "missing": i.missing, "w": i.w, "h": i.h})
}

fn modal_name(m: &Option<Modal>) -> Option<&'static str> {
    m.as_ref().map(|m| match m {
        Modal::Import { .. } => "import",
        Modal::SaveFilter { .. } => "save_filter",
        Modal::RenameFilter { .. } => "rename_filter",
        Modal::EmptyTrash { .. } => "empty_trash",
        Modal::ForgetMissing { .. } => "forget_missing",
        Modal::Help => "help",
        Modal::Rename { .. } => "rename",
        Modal::Export { .. } => "export",
        Modal::TagRename { .. } => "tag_rename",
        Modal::TagDelete { .. } => "tag_delete",
        Modal::AlbumRename { .. } => "album_rename",
        Modal::AlbumNew { .. } => "album_new",
        Modal::AlbumDelete { .. } => "album_delete",
        Modal::Settings { .. } => "settings",
    })
}

fn mods(b: &Value) -> Modifiers {
    let f = |k: &str| b.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    Modifiers { ctrl: f("ctrl"), command: f("ctrl"), shift: f("shift"), alt: f("alt"), ..Default::default() }
}

fn parse_key(k: &str) -> Option<Key> {
    Key::from_name(k).or_else(|| Key::from_name(k.trim_start_matches("Num")))
}

impl App {
    fn push_inject(&mut self, events: Vec<egui::Event>, mods: Modifiers) {
        self.inject.push_back(InjectStep { events, mods: Some(mods), reply: None });
    }

    fn mouse_steps(&mut self, b: &Value) -> Result<(), String> {
        let num = |k: &str| b.get(k).and_then(|v| v.as_f64()).map(|v| v as f32);
        let op = b.get("op").and_then(|v| v.as_str()).unwrap_or("click");
        let m = mods(b);
        let p = match (num("x"), num("y")) {
            (Some(x), Some(y)) => Pos2::new(x, y),
            _ => self.inject_ptr,
        };
        let btn = if op == "right" { egui::PointerButton::Secondary } else { egui::PointerButton::Primary };
        let press = |pos: Pos2, pressed: bool| egui::Event::PointerButton { pos, button: btn, pressed, modifiers: m };
        match op {
            "move" => self.push_inject(vec![egui::Event::PointerMoved(p)], m),
            "down" => {
                self.push_inject(vec![egui::Event::PointerMoved(p)], m);
                self.push_inject(vec![press(p, true)], m);
            }
            "up" => self.push_inject(vec![egui::Event::PointerMoved(p), press(p, false)], m),
            "click" | "right" => {
                self.push_inject(vec![egui::Event::PointerMoved(p)], m);
                self.push_inject(vec![press(p, true)], m);
                self.push_inject(vec![press(p, false)], m);
            }
            "dblclick" => {
                self.push_inject(vec![egui::Event::PointerMoved(p)], m);
                for _ in 0..2 {
                    self.push_inject(vec![press(p, true)], m);
                    self.push_inject(vec![press(p, false)], m);
                }
            }
            "drag" => {
                let to = b.get("to").and_then(|v| v.as_array()).filter(|a| a.len() == 2).ok_or("drag needs to:[x,y]")?;
                let q = Pos2::new(to[0].as_f64().unwrap_or(0.0) as f32, to[1].as_f64().unwrap_or(0.0) as f32);
                let steps = b.get("steps").and_then(|v| v.as_u64()).unwrap_or(8).clamp(1, 60) as usize;
                self.push_inject(vec![egui::Event::PointerMoved(p)], m);
                self.push_inject(vec![press(p, true)], m);
                for k in 1..=steps {
                    let t = k as f32 / steps as f32;
                    self.push_inject(vec![egui::Event::PointerMoved(p + (q - p) * t)], m);
                }
                self.push_inject(vec![press(q, false)], m);
                self.inject_ptr = q;
                return Ok(());
            }
            _ => return Err(format!("unknown mouse op '{op}'")),
        }
        self.inject_ptr = p;
        Ok(())
    }

    pub(super) fn handle_ctl(&mut self, ctx: &egui::Context) {
        while let Ok(req) = self.ctl_rx.try_recv() {
            let b = &req.body;
            let cmd = b.get("cmd").and_then(|c| c.as_str()).unwrap_or("").to_string();
            let s = |k: &str| b.get(k).and_then(|v| v.as_str()).map(|v| v.to_string());
            let flag = |k: &str| b.get(k).and_then(|v| v.as_bool());
            let out: Option<Value> = match cmd.as_str() {
                "state" => Some(self.state_json()),
                "query" => {
                    self.set_query(s("q").unwrap_or_default());
                    Some(self.state_json())
                }
                "list" => {
                    let lim = b.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
                    let v: Vec<Value> = self.shown.iter().take(lim).map(|&i| img_json(&self.db.images[i])).collect();
                    Some(json!({"ok": true, "shown": self.shown.len(), "items": v}))
                }
                "select" => {
                    let ids: Vec<String> = b.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                    self.select_ids(&ids);
                    if let Some(a) = b.get("index").and_then(|v| v.as_array()) {
                        for p in a.iter().filter_map(|x| x.as_u64()) {
                            if let Some(&i) = self.shown.get(p as usize) {
                                self.sel.insert(self.db.images[i].id.clone());
                            }
                        }
                    }
                    if flag("all").unwrap_or(false) {
                        self.sel = self.shown.iter().map(|&i| self.db.images[i].id.clone()).collect();
                    }
                    let first = self.shown.iter().position(|&i| self.sel.contains(&self.db.images[i].id));
                    if first.is_some() {
                        self.focus = first;
                        self.anchor = first;
                        self.scroll_to = first;
                    }
                    Some(self.state_json())
                }
                "score" => {
                    let n = b.get("n").and_then(|v| v.as_u64()).unwrap_or(0).min(6) as u8;
                    self.set_score(n);
                    Some(self.state_json())
                }
                "tag" => {
                    for t in b.get("add").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
                        if let Some(t) = t.as_str() {
                            self.add_tag(t);
                        }
                    }
                    for t in b.get("remove").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
                        if let Some(t) = t.as_str() {
                            self.remove_tag(&clean_tag(t));
                        }
                    }
                    Some(self.state_json())
                }
                "trash" => {
                    self.set_trash(flag("on").unwrap_or(true));
                    Some(self.state_json())
                }
                "undo" => {
                    self.do_undo();
                    Some(self.state_json())
                }
                "cmp_add" => {
                    self.cmp_add_selected(flag("clear").unwrap_or(false));
                    Some(self.state_json())
                }
                "cmp_row" => {
                    let op = match s("op").as_deref() {
                        Some("remove") => Some(CmpOp::Remove),
                        Some("up") => Some(CmpOp::Up),
                        Some("down") => Some(CmpOp::Down),
                        Some("top") => Some(CmpOp::Top),
                        Some("bottom") => Some(CmpOp::Bottom),
                        _ => None,
                    };
                    match (op, b.get("row").and_then(|v| v.as_u64())) {
                        (Some(op), Some(r)) => {
                            self.cmp_row_op(r as usize, op);
                            Some(self.state_json())
                        }
                        _ => Some(json!({"ok": false, "error": "row and op (remove|up|down|top|bottom) needed"})),
                    }
                }
                "view" => {
                    match s("mode").as_deref() {
                        Some("grid") => self.view = View::Grid,
                        Some("viewer") => {
                            if self.focus.is_some() {
                                self.open_viewer(self.view);
                            }
                        }
                        Some("compare") => {
                            if self.view != View::Compare {
                                self.toggle_cmp();
                            }
                        }
                        Some("matrix") => {
                            self.back_view = View::Grid;
                            self.view = View::Matrix;
                        }
                        Some("albums") => self.view = View::Albums,
                        _ => {}
                    }
                    if let Some(z) = b.get("zoom").and_then(|v| v.as_f64()) {
                        self.zp.zoom = (z as f32).clamp(1.0, 40.0);
                    }
                    if let Some(c) = b.get("center").and_then(|v| v.as_array()) {
                        if c.len() == 2 {
                            self.zp.center = vec2(c[0].as_f64().unwrap_or(0.5) as f32, c[1].as_f64().unwrap_or(0.5) as f32);
                        }
                    }
                    Some(self.state_json())
                }
                "key" => match s("key").as_deref().and_then(parse_key) {
                    Some(k) => {
                        self.on_key(k, mods(b));
                        Some(self.state_json())
                    }
                    None => Some(json!({"ok": false, "error": "unknown key (egui names: ArrowRight, Enter, Escape, A, F2, Num1 ...)"})),
                },
                // a real key event through egui (reaches text fields, menus, modals)
                "press" => match s("key").as_deref().and_then(parse_key) {
                    Some(k) => {
                        let m = mods(b);
                        let ev = |pressed| egui::Event::Key { key: k, physical_key: None, pressed, repeat: false, modifiers: m };
                        self.push_inject(vec![ev(true)], m);
                        self.inject.push_back(InjectStep { events: vec![ev(false)], mods: Some(m), reply: Some(req.reply.clone()) });
                        None
                    }
                    None => Some(json!({"ok": false, "error": "unknown key"})),
                },
                // IME steps for testing Korean input: "p:<preedit>", "c:<commit>" (one frame each)
                "ime" => {
                    let steps: Vec<String> = b.get("steps").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                    let n = steps.len();
                    for (k, st) in steps.into_iter().enumerate() {
                        let ev = if let Some(t) = st.strip_prefix("p:") {
                            egui::ImeEvent::Preedit { text: t.to_string(), active_range_chars: None }
                        } else {
                            egui::ImeEvent::Commit(st.strip_prefix("c:").unwrap_or(&st).to_string())
                        };
                        let reply = (k + 1 == n).then(|| req.reply.clone());
                        self.inject.push_back(InjectStep { events: vec![egui::Event::Ime(ev)], mods: Some(Modifiers::NONE), reply });
                    }
                    if n == 0 { Some(json!({"ok": false, "error": "steps: [\"p:ㅎ\", \"p:한\", \"c:한\"]"})) } else { None }
                }
                "type" => {
                    let t = s("text").unwrap_or_default();
                    self.inject.push_back(InjectStep { events: vec![egui::Event::Text(t)], mods: Some(Modifiers::NONE), reply: Some(req.reply.clone()) });
                    None
                }
                "mouse" => match self.mouse_steps(b) {
                    Ok(()) => {
                        // one empty frame afterwards so the reply sees the result
                        self.inject.push_back(InjectStep { events: vec![], mods: Some(Modifiers::NONE), reply: Some(req.reply.clone()) });
                        None
                    }
                    Err(e) => Some(json!({"ok": false, "error": e})),
                },
                "wheel" => {
                    let x = b.get("x").and_then(|v| v.as_f64()).map(|v| v as f32).unwrap_or(self.inject_ptr.x);
                    let y = b.get("y").and_then(|v| v.as_f64()).map(|v| v as f32).unwrap_or(self.inject_ptr.y);
                    let dy = b.get("dy").and_then(|v| v.as_f64()).unwrap_or(-120.0) as f32;
                    let m = mods(b);
                    let p = Pos2::new(x, y);
                    self.inject_ptr = p;
                    self.push_inject(vec![egui::Event::PointerMoved(p)], m);
                    self.inject.push_back(InjectStep {
                        events: vec![egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: vec2(0.0, dy),
                            phase: egui::TouchPhase::Move,
                            source: egui::MouseWheelSource::Wheel,
                            modifiers: m,
                        }],
                        mods: Some(m),
                        reply: None,
                    });
                    self.inject.push_back(InjectStep { events: vec![], mods: Some(Modifiers::NONE), reply: Some(req.reply.clone()) });
                    None
                }
                "thumb" => {
                    if let Some(v) = b.get("size").and_then(|v| v.as_f64()) {
                        self.settings.thumb_size = (v as f32).clamp(80.0, 420.0);
                    }
                    Some(self.state_json())
                }
                "window" => {
                    if let (Some(w), Some(h)) = (b.get("w").and_then(|v| v.as_f64()), b.get("h").and_then(|v| v.as_f64())) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(w as f32, h as f32)));
                    }
                    Some(json!({"ok": true}))
                }
                "layout" => {
                    let m: serde_json::Map<String, Value> = self
                        .layout
                        .iter()
                        .map(|(k, r)| (k.clone(), json!([r.min.x.round(), r.min.y.round(), r.max.x.round(), r.max.y.round()])))
                        .collect();
                    Some(json!({"ok": true, "window": [self.window_size.x, self.window_size.y], "rects": m}))
                }
                "screenshot" => {
                    let path = s("path").map(PathBuf::from).unwrap_or_else(|| db::data_dir().join("shot.png"));
                    let reply = req.reply.clone();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img: Arc<egui::ColorImage>| {
                        let [w, h] = img.size;
                        let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                        let res = image::RgbaImage::from_raw(w as u32, h as u32, raw).map(|b| b.save(&path));
                        let v = match res {
                            Some(Ok(())) => json!({"ok": true, "path": path.display().to_string(), "size": [w, h]}),
                            Some(Err(e)) => json!({"ok": false, "error": e.to_string()}),
                            None => json!({"ok": false, "error": "bad image"}),
                        };
                        let _ = reply.send(v);
                    })));
                    None
                }
                "rescan" => {
                    self.start_scan(false);
                    Some(json!({"ok": true}))
                }
                "import" => match s("from") {
                    Some(f) => {
                        self.start_import(vec![PathBuf::from(f)], flag("move").unwrap_or(false));
                        Some(json!({"ok": true, "note": "running; see state.import"}))
                    }
                    None => Some(json!({"ok": false, "error": "from missing"})),
                },
                "albums" => {
                    let v: Vec<Value> = self
                        .album_rows()
                        .into_iter()
                        .map(|(id, name, depth)| {
                            let parent = self.db.album(id).map(|a| a.parent).unwrap_or(0);
                            json!({"id": id, "name": name, "depth": depth, "parent": parent, "n": self.album_counts.get(&id).copied().unwrap_or(0)})
                        })
                        .collect();
                    let cur = match self.cur_album() {
                        Some(Some(id)) => json!(id),
                        Some(None) => json!("none"),
                        None => Value::Null,
                    };
                    Some(json!({"ok": true, "albums": v, "unfiled": self.unfiled_n, "cur": cur, "clip": self.album_clip.as_ref().map(|c| json!({"n": c.ids.len(), "cut": c.cut}))}))
                }
                "album_new" => {
                    let parent = b.get("parent").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let id = self.db.new_album(&s("name").unwrap_or_else(|| "새 앨범".into()), parent);
                    self.dirty = true;
                    self.mark_changed();
                    Some(json!({"ok": true, "id": id, "name": self.album_label(Some(id))}))
                }
                "album_rename" => {
                    let id = b.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    match self.db.rename_album(id, &s("name").unwrap_or_default()) {
                        Some(n) => {
                            self.dirty = true;
                            Some(json!({"ok": true, "name": n}))
                        }
                        None => Some(json!({"ok": false, "error": "no such album"})),
                    }
                }
                "album_delete" => {
                    let id = b.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    self.delete_album(id);
                    Some(json!({"ok": true}))
                }
                "album_sel" => {
                    // select albums in the album view (ids), like clicking their cells
                    self.album_sel = b.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64()).map(|x| x as u32).filter(|&x| self.db.album(x).is_some()).collect()).unwrap_or_default();
                    if b.get("only").and_then(|v| v.as_bool()).unwrap_or(true) {
                        self.sel.clear();
                    }
                    Some(json!({"ok": true, "n": self.album_sel.len()}))
                }
                "album_put" => {
                    let ids = self.selected_ids();
                    let to = b.get("id").and_then(|v| v.as_u64()).map(|x| x as u32);
                    self.add_to_album(&ids, to);
                    Some(json!({"ok": true, "n": ids.len()}))
                }
                "album_open" => {
                    let a = match b.get("id") {
                        Some(Value::String(x)) if x == "none" => Some(None),
                        Some(v) if v.as_u64().is_some() => Some(Some(v.as_u64().unwrap() as u32)),
                        _ => None,
                    };
                    self.open_album(a);
                    Some(json!({"ok": true}))
                }
                "drop" => {
                    // the same path as files dropped on the window from Explorer
                    let files: Vec<PathBuf> = b.get("files").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(PathBuf::from).collect()).unwrap_or_default();
                    if files.is_empty() {
                        Some(json!({"ok": false, "error": "files missing"}))
                    } else {
                        self.inject_drop.extend(files);
                        self.ctx.request_repaint();
                        Some(json!({"ok": true, "note": "running; see state.import"}))
                    }
                }
                "save_filter" => {
                    let name = s("name").unwrap_or_default();
                    let q = query::explicit(&s("q").unwrap_or_else(|| self.query_text.clone()), self.settings.search_or);
                    if name.is_empty() {
                        Some(json!({"ok": false, "error": "name missing"}))
                    } else {
                        self.db.saved.retain(|f| f.name != name);
                        self.db.saved.push(db::SavedFilter { name, query: q });
                        self.dirty = true;
                        Some(json!({"ok": true}))
                    }
                }
                "notice" => {
                    self.notice(s("text").unwrap_or_default(), false);
                    Some(json!({"ok": true}))
                }
                "rename" => {
                    let ids = self.selected_ids();
                    if ids.is_empty() {
                        Some(json!({"ok": false, "error": "nothing selected"}))
                    } else {
                        let upload = flag("upload").unwrap_or(false);
                        let pattern = s("pattern");
                        let start = b.get("start").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                        let mut names = Vec::new();
                        for (k, id) in ids.iter().enumerate() {
                            let Some(im) = self.img(id) else { continue };
                            let ext = im.name().rfind('.').map(|i| im.name()[i..].to_string()).unwrap_or_default();
                            let new = if upload {
                                format!("{}{ext}", db::upload_name(k as u64))
                            } else if let Some(p) = &pattern {
                                let hashes = p.chars().filter(|c| *c == '#').count().max(1);
                                let n = format!("{:0w$}", start + k as u32, w = hashes);
                                let body = if p.contains('#') { p.replacen(&"#".repeat(p.chars().filter(|c| *c == '#').count()), &n, 1) } else { format!("{p}_{n}") };
                                format!("{body}{ext}")
                            } else {
                                let n = s("name").unwrap_or_default();
                                if db::is_image(std::path::Path::new(&n)) { n } else { format!("{n}{ext}") }
                            };
                            names.push((id.clone(), new));
                        }
                        if let Some(bad) = names.iter().find_map(|(_, n)| db::bad_name(n)) {
                            Some(json!({"ok": false, "error": bad}))
                        } else {
                            match self.rename_images(names) {
                                Ok(n) => Some(json!({"ok": true, "renamed": n, "state": self.state_json()})),
                                Err(e) => Some(json!({"ok": false, "error": e})),
                            }
                        }
                    }
                }
                "export" => {
                    let ids = self.selected_ids();
                    match s("dir") {
                        None => Some(json!({"ok": false, "error": "dir missing"})),
                        Some(_) if ids.is_empty() => Some(json!({"ok": false, "error": "nothing selected"})),
                        Some(d) if db::path_inside(std::path::Path::new(&d), &db::src_dir()) => Some(json!({"ok": false, "error": "imgs 안으로는 내보낼 수 없음"})),
                        Some(d) => {
                            let q = b.get("quality").and_then(|v| v.as_u64()).unwrap_or(92).clamp(1, 100) as u8;
                            let f = match s("fmt").as_deref() {
                                Some("png") => ops::Fmt::Png,
                                Some("keep") => ops::Fmt::Keep,
                                _ => ops::Fmt::Jpg(q),
                            };
                            self.start_export(ids, PathBuf::from(d), f, flag("upload").unwrap_or(false), s("tag").unwrap_or_default());
                            Some(json!({"ok": true, "note": "running; see state.export"}))
                        }
                    }
                }
                "panel" => {
                    if let Some(v) = flag("show_left") {
                        self.settings.show_left = v;
                    }
                    if let Some(v) = flag("show_right") {
                        self.settings.show_right = v;
                    }
                    Some(self.state_json())
                }
                "modal" => {
                    if flag("close").unwrap_or(false) {
                        self.modal = None;
                    }
                    Some(json!({"ok": true, "modal": modal_name(&self.modal)}))
                }
                "focus" => {
                    if !self.offscreen {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    Some(json!({"ok": true}))
                }
                "save" => {
                    self.save_now();
                    Some(json!({"ok": true}))
                }
                "quit" => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    Some(json!({"ok": true}))
                }
                _ => Some(json!({"ok": false, "error": format!("unknown cmd '{cmd}'"), "cmds": CMDS})),
            };
            if let Some(v) = out {
                let _ = req.reply.send(v);
            }
            ctx.request_repaint();
        }
    }

    pub(super) fn state_json(&self) -> Value {
        let sel = self.picked_pictures();
        let mut album_sel: Vec<u32> = self.album_sel.iter().copied().collect();
        album_sel.sort_unstable();
        let refs: HashMap<String, String> = self.db.albums.iter().map(|a| (a.id.to_string(), self.album_ref(a.id))).collect();
        let mut v = self.state_json_main(sel, album_sel);
        v["album_refs"] = json!(refs);
        v["viewer_text"] = json!(self.settings.viewer_text);
        v["kind"] = json!([self.settings.kind_wf, self.settings.kind_plain]);
        v["per_folder"] = json!(self.settings.per_folder);
        v["focus_asked"] = json!(self.focus_asked.load(std::sync::atomic::Ordering::Relaxed));
        v["ctl_setting"] = json!(self.settings.ctl);
        v["lang"] = json!(self.settings.lang);
        v
    }

    fn state_json_main(&self, sel: Vec<String>, album_sel: Vec<u32>) -> Value {
        json!({
            "album_sel": album_sel,
            "album_focus": self.album_focus,
            "album_lead": self.album_lead(),
            "ok": true, "port": self.ctl_port,
            "root": db::root_dir().display().to_string(),
            "query": self.query_text,
            "sort": self.sort.key(),
            "view": self.view.key(),
            "modal": modal_name(&self.modal),
            "modal_text": match &self.modal {
                Some(Modal::TagRename { new, .. }) | Some(Modal::AlbumRename { new, .. }) | Some(Modal::AlbumNew { new, .. }) => new.clone(),
                _ => String::new(),
            },
            "total": self.db.images.len(),
            "shown": self.shown.len(),
            "selected": sel.len(),
            "selected_ids": sel.iter().take(30).collect::<Vec<_>>(),
            "focus": self.focus,
            "focus_img": self.focus_img().map(img_json),
            "cols": self.cols,
            "scroll": self.scroll_offset,
            "thumb": self.settings.thumb_size,
            "scan": self.scan_progress.map(|(d, t)| json!([d, t])),
            "import": self.import_progress.map(|(d, t)| json!([d, t])),
            "export": self.export_progress.map(|(d, t)| json!([d, t])),
            "notice": self.notices.back().map(|n| n.text.clone()),
            "compare": self.settings.cmp_rows.iter().map(|r| json!({"label": r.label, "ids": r.ids})).collect::<Vec<_>>(),
            "cmp_focus": [self.cmp_focus.0, self.cmp_focus.1],
            "zoom": self.zp.zoom,
            "window": [self.window_size.x, self.window_size.y],
            "panels": {"left": self.settings.show_left.then_some(self.settings.left_w), "right": self.settings.show_right.then_some(self.settings.right_w)},
            "fullscreen": self.fullscreen,
            "tag_input": self.tag_input,
            "trash_n": self.trash_n, "missing_n": self.missing_n,
            "watch": self.settings.watch_dirs,
            "tags": self.tag_counts.iter().take(40).map(|(t, n)| json!([t, n])).collect::<Vec<_>>(),
            "filters": self.db.saved.iter().map(|f| json!([f.name, f.query])).collect::<Vec<_>>(),
        })
    }
}
