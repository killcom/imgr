//! imgr window. Search + side filters on the left, picture grid in the middle,
//! details / score / hashtags on the right. Viewer, compare, matrix (seed x
//! preset table) replace the grid. Everything can also be driven
//! through ctl (see ctl.rs and app/ctl_cmds.rs).
#![allow(deprecated)]

mod album_ui;
mod ctl_cmds;
mod grid;
mod modals;
mod panels;
mod views;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Key, Modifiers, Pos2, Rect, Stroke, Vec2, vec2};
use serde_json::Value;

use crate::ctl;
use crate::i18n::tr;
use crate::db::{self, Db, Img};
use crate::ops;
use crate::query::{self, Query, Sort};
use crate::settings::Settings;
use crate::thumbs::{self, Kind, Loader};
use crate::winshell;

const BG: Color32 = Color32::from_rgb(22, 22, 24);
const PANEL: Color32 = Color32::from_rgb(30, 30, 33);
const CELL_HOVER: Color32 = Color32::from_rgb(40, 42, 48);
const SEL: Color32 = Color32::from_rgb(38, 58, 88);
const SEL_STROKE: Color32 = Color32::from_rgb(90, 125, 170);
const FOCUS_STROKE: Color32 = Color32::from_rgb(200, 200, 210);
const TEXT: Color32 = Color32::from_rgb(205, 205, 210);
const DIM: Color32 = Color32::from_rgb(130, 130, 138);
const ACCENT: Color32 = Color32::from_rgb(110, 150, 200);
const ERR: Color32 = Color32::from_rgb(205, 115, 105);
const OK: Color32 = Color32::from_rgb(130, 175, 130);
const TAG_BG: Color32 = Color32::from_rgb(44, 52, 66);

const MAX_THUMBS: usize = 900;
const MAX_BIGS: usize = 10;
const BIG_PX: u32 = 2400;
const PREVIEW_PX: u32 = 1100;
const RESCAN_EVERY: Duration = Duration::from_secs(45);
const WATCH_EVERY: Duration = Duration::from_secs(60);

fn score_color(s: u8) -> Color32 {
    match s {
        1 => Color32::from_rgb(120, 70, 70),
        2 => Color32::from_rgb(135, 100, 70),
        3 => Color32::from_rgb(125, 125, 80),
        4 => Color32::from_rgb(80, 125, 90),
        5 => Color32::from_rgb(70, 115, 150),
        6 => Color32::from_rgb(140, 95, 160),
        _ => Color32::from_rgb(60, 60, 66),
    }
}

enum Slot {
    Pending(Instant),
    Ready(egui::TextureHandle, u64),
    /// load failed at this time; tried again after 30 s
    Failed(Instant),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Grid,
    Viewer,
    Compare,
    Matrix,
    /// albums as folders: the top shows 앨범 없음 + top-level albums
    Albums,
}

impl View {
    fn key(self) -> &'static str {
        match self {
            View::Grid => "grid",
            View::Viewer => "viewer",
            View::Compare => "compare",
            View::Matrix => "matrix",
            View::Albums => "albums",
        }
    }
}

enum Modal {
    Import { dir: String, mv: bool, watch_new: String },
    SaveFilter { name: String },
    RenameFilter { idx: usize, name: String },
    EmptyTrash { n: usize },
    ForgetMissing { ids: Vec<String> },
    Help,
    Rename { ids: Vec<String>, name: String, fresh: bool, upload: bool, pattern: String, start: u32, error: String },
    Export { ids: Vec<String>, dir: String, fmt: String, quality: u8, upload: bool, tag: String },
    TagRename { old: String, new: String },
    TagDelete { tag: String, n: usize },
    /// name a new album / rename one
    AlbumRename { id: u32, new: String },
    /// a new album under `parent` (0 = top); made only on 만들기. `put`: pictures to put in it
    AlbumNew { parent: u32, new: String, put: Vec<String> },
    AlbumDelete { ids: Vec<u32> },
    /// ⚙ 설정
    Settings { per_folder: String, ctl: bool },
}

enum ScanMsg {
    Progress(usize, usize),
    Done(Vec<db::ScanItem>, usize),
}

enum ImportMsg {
    Progress(usize, usize),
    /// report, watch keys to mark seen, watch keys still present (None = manual import)
    Done(db::ImportReport, Vec<(String, (u64, u64))>, Option<HashSet<String>>),
}

enum ExportMsg {
    Progress(usize, usize),
    Done(ops::ExportReport),
}

struct Notice {
    text: String,
    error: bool,
    at: Instant,
}

/// Pictures copied (Ctrl+C) or cut (Ctrl+X) for pasting into an album.
struct AlbumClip {
    ids: Vec<String>,
    cut: bool,
    /// album shown when cut (None = not in an album view: cut from every album)
    from: Option<Option<u32>>,
    /// albums cut in the album view (Ctrl+V puts them under the album shown)
    albums: Vec<u32>,
}

/// One picture's editable state before an edit.
struct UndoItem {
    id: String,
    score: u8,
    tags: Vec<String>,
    trash: bool,
    preset: String,
    preset_set: bool,
    albums: Vec<u32>,
}

/// One undo step: previous score/tags/trash/preset of some pictures.
struct UndoStep {
    label: String,
    items: Vec<UndoItem>,
}

#[derive(Default)]
struct ZoomPan {
    /// 1.0 = fit
    zoom: f32,
    /// centre of the view in image fractions (0.5, 0.5 = middle)
    center: Vec2,
    /// show at 1 image pixel = 1 screen pixel
    actual: bool,
}

impl ZoomPan {
    fn reset(&mut self) {
        self.zoom = 1.0;
        self.center = vec2(0.5, 0.5);
        self.actual = false;
    }
}

/// A file handed in by the ctl "drop" command, seen by egui like an Explorer drop.
#[derive(Debug)]
struct CtlDrop(PathBuf);
impl egui::DroppedFile for CtlDrop {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Events injected by ctl, one group per frame.
struct InjectStep {
    events: Vec<egui::Event>,
    /// None = leave the modifier state alone
    mods: Option<Modifiers>,
    reply: Option<Sender<Value>>,
}

pub struct App {
    db: Db,
    /// id -> index in db.images
    id_index: HashMap<String, usize>,
    dirty: bool,
    last_save: Instant,
    settings: Settings,
    query_text: String,
    sort: Sort,
    shown: Vec<usize>,
    pos_of: HashMap<String, usize>,
    shown_gen: u64,
    sel: HashSet<String>,
    focus: Option<usize>,
    anchor: Option<usize>,
    view: View,
    /// where Esc goes back to from the viewer
    back_view: View,
    cmp_ids: Vec<String>,
    cmp_focus: usize,
    zp: ZoomPan,
    loader: Loader,
    tex: HashMap<(String, Kind), Slot>,
    frame_no: u64,
    cols: usize,
    aspect: f32,
    row_h: f32,
    scroll_to: Option<usize>,
    scroll_offset: f32,
    view_h: f32,
    force_scroll: Option<f32>,
    scan_rx: Option<Receiver<ScanMsg>>,
    scan_progress: Option<(usize, usize)>,
    scan_quiet: bool,
    last_scan: Instant,
    import_rx: Option<Receiver<ImportMsg>>,
    import_progress: Option<(usize, usize)>,
    pending_known: db::Known,
    /// pictures dropped / chosen while another import ran: imported right after it
    import_queue: Vec<(Vec<PathBuf>, bool, Option<Option<u32>>)>,
    /// album the running import's pictures go into (Some(None) = none: nothing to do)
    import_album: Option<Option<u32>>,
    /// (picture id, album) waiting for the picture's record (imported, not scanned yet)
    pending_album: Vec<(String, u32)>,
    export_rx: Option<Receiver<ExportMsg>>,
    export_progress: Option<(usize, usize)>,
    export_tag: String,
    notices: VecDeque<Notice>,
    ctl_rx: Receiver<ctl::Request>,
    ctl_port: Option<u16>,
    inject: VecDeque<InjectStep>,
    inject_replies: Vec<Sender<Value>>,
    inject_ptr: Pos2,
    /// files a ctl "drop" hands to the next frame as if dropped on the window
    inject_drop: Vec<PathBuf>,
    layout: HashMap<String, Rect>,
    tag_input: String,
    focus_tag_input: bool,
    focus_search: bool,
    modal: Option<Modal>,
    tag_counts: Vec<(String, usize)>,
    /// pictures directly in each album (live, not in the trash)
    album_counts: HashMap<u32, usize>,
    /// pictures in each album or any of its sub albums (each picture once) — 검색 뷰 counts
    album_tree_counts: HashMap<u32, usize>,
    /// live pictures with / without workflow info (태그 여부)
    kind_counts: (usize, usize),
    /// albums as last told to the search (id, name, parent): see `sync_album_names`
    album_sig: Vec<(u32, String, u32)>,
    /// pictures in no album
    unfiled_n: usize,
    /// albums folded in the side panel
    album_folded: HashSet<u32>,
    /// Ctrl+C / Ctrl+X of pictures, for Ctrl+V into an album
    album_clip: Option<AlbumClip>,
    /// pictures being dragged inside the window (to an album); leaving the window turns it into a drag-out
    pic_drag: Option<Vec<String>>,
    /// album drop spots of this frame (None = 앨범 없음, Some(0) = the top)
    album_drop: Vec<(Option<u32>, Rect)>,
    /// albums selected in the album view (they sit in the grid like folders)
    album_sel: HashSet<u32>,
    /// grid index of the last plain / Ctrl click on an album (Shift range start)
    album_anchor: Option<usize>,
    /// album view: the album cell that has the keyboard cursor (grid index; None = the cursor is on a picture)
    album_focus: Option<usize>,
    /// time a plain click opened an album cell (the 2nd click of a double click is then ignored)
    album_click_open_t: f64,
    /// album view: scroll this grid index (album cells first) into view
    scroll_to_cell: Option<usize>,
    /// albums being dragged together with `pic_drag`
    album_drag: Vec<u32>,
    /// cover picture of each album tile (None = 앨범 없음), filled by the album view
    album_cover: HashMap<Option<u32>, String>,
    /// the view of the last frame (to see the album view being entered)
    prev_view: View,
    /// width the top bar's buttons right of the search box took last frame
    top_rest_w: f32,
    ckpt_counts: Vec<(String, usize)>,
    lora_counts: Vec<(String, usize)>,
    /// the corner handle under the preview is being dragged (panel width follows it exactly)
    corner_drag: bool,
    folder_counts: Vec<(String, usize)>,
    score_counts: [usize; 7],
    missing_n: usize,
    trash_n: usize,
    counts_dirty: bool,
    undo: Vec<UndoStep>,
    ctx: egui::Context,
    window_size: Vec2,
    window_pos: Option<Pos2>,
    maximized: bool,
    rand_seed: u64,
    marquee: Option<grid::Marquee>,
    press: Option<(Pos2, usize)>,
    mx_cache: Option<views::Matrix>,
    mx_hidden: HashSet<String>,
    mx_focus: Option<(usize, usize)>,
    /// last right click in the matrix was on a picture
    mx_menu: bool,
    watch_seen: ops::Seen,
    last_watch: Instant,
    fullscreen: bool,
    offscreen: bool,
    /// times a second start asked for this window (see `winshell::on_focus_request`)
    focus_asked: Arc<AtomicUsize>,
    focus_seen: usize,
    first_frame: bool,
    /// bumped by renames / record removals; a scan started before is re-run
    mut_gen: u64,
    scan_gen: u64,
    /// imgr.db writer thread (changed rows, written in order; optional ack for exit)
    save_tx: Sender<(crate::store::Delta, Option<Sender<()>>)>,
    save_err: Arc<std::sync::Mutex<Option<String>>>,
    /// ids re-hash needed (db from an older id version)
    rehash: bool,
}

/// Added to every text size (egui styles and the texts painted by hand).
pub const UI_FONT_PLUS: f32 = 2.0;

pub fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for f in ["C:\\Windows\\Fonts\\malgun.ttf", "C:\\Windows\\Fonts\\gulim.ttc"] {
        if let Ok(bytes) = std::fs::read(f) {
            fonts.font_data.insert("korean".into(), Arc::new(egui::FontData::from_owned(bytes)));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push("korean".into());
            }
            break;
        }
    }
    ctx.set_fonts(fonts);
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = Color32::from_rgb(34, 34, 38);
    v.extreme_bg_color = Color32::from_rgb(18, 18, 20);
    v.faint_bg_color = Color32::from_rgb(32, 32, 35);
    v.selection.bg_fill = SEL;
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(160, 180, 210));
    v.hyperlink_color = ACCENT;
    v.override_text_color = Some(TEXT);
    ctx.set_visuals_of(egui::Theme::Dark, v.clone());
    ctx.set_visuals_of(egui::Theme::Light, v);
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
    ctx.global_style_mut(|s| {
        // all text 2 px bigger than egui's defaults
        for (_, f) in s.text_styles.iter_mut() {
            f.size += UI_FONT_PLUS;
        }
        s.spacing.button_padding = vec2(8.0, 4.0);
        s.spacing.item_spacing = vec2(6.0, 5.0);
        s.interaction.tooltip_delay = 0.45;
    });
}

fn human_size(n: u64) -> String {
    let n = n as f64;
    if n >= 1073741824.0 {
        format!("{:.2} GB", n / 1073741824.0)
    } else if n >= 1048576.0 {
        format!("{:.1} MB", n / 1048576.0)
    } else {
        format!("{:.0} KB", n / 1024.0)
    }
}

fn fmt_time(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

fn clean_tag(s: &str) -> String {
    s.trim().trim_start_matches('#').trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join("_")
}

fn mix(id: &str, seed: u64) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325 ^ seed;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        let settings = Settings::load();
        db::set_per_folder(settings.per_folder);
        crate::i18n::set(&settings.lang);
        let (db, db_warn) = Db::load();
        let rehash = db.version < db::DB_VERSION && !db.images.is_empty();
        let save_err: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let (save_tx, save_rx) = channel::<(crate::store::Delta, Option<Sender<()>>)>();
        {
            let err = save_err.clone();
            let _ = std::thread::Builder::new().name("db-writer".into()).spawn(move || {
                let mut conn: Option<rusqlite::Connection> = None;
                while let Ok((delta, ack)) = save_rx.recv() {
                    // every delta is written, in order (each holds only its own changes)
                    let r = (|| -> Result<(), String> {
                        if conn.is_none() {
                            let _ = std::fs::create_dir_all(db::data_dir());
                            conn = Some(crate::store::open(&crate::store::path()).map_err(|e| e.to_string())?);
                        }
                        let c = conn.as_mut().expect("opened above");
                        if !delta.is_empty() {
                            crate::store::apply(c, &delta).map_err(|e| e.to_string())?;
                        }
                        crate::store::daily_backup(c);
                        Ok(())
                    })();
                    if r.is_err() {
                        conn = None;
                    }
                    if let Err(e) = r {
                        *err.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
                    }
                    if let Some(a) = ack {
                        let _ = a.send(());
                    }
                }
            });
        }
        let loader = Loader::new(cc.egui_ctx.clone(), db::data_dir().join("thumbs"));
        let (ctl_tx, ctl_rx) = channel();
        // script control: only when asked (⚙ 설정 or IMGR_CTL=1)
        let ctl_on = settings.ctl || std::env::var_os("IMGR_CTL").is_some_and(|v| v != "0");
        let ctl_port = if ctl_on { ctl::start(cc.egui_ctx.clone(), ctl_tx) } else { None };
        match ctl_port {
            Some(p) => {
                let _ = std::fs::create_dir_all(db::data_dir());
                let _ = std::fs::write(db::data_dir().join("ctl.port"), p.to_string());
            }
            None => {
                let _ = std::fs::remove_file(db::data_dir().join("ctl.port"));
            }
        }
        // a second start of imgr on this root brings this window to the front
        let focus_asked = Arc::new(AtomicUsize::new(0));
        {
            let fa = focus_asked.clone();
            let c = cc.egui_ctx.clone();
            winshell::on_focus_request(&db::root_dir().display().to_string().to_lowercase(), move || {
                fa.fetch_add(1, Ordering::Relaxed);
                c.request_repaint();
            });
        }
        let sort = Sort::from_key(&settings.sort).unwrap_or_default();
        let q = settings.query.clone();
        let mut app = App {
            db,
            id_index: HashMap::new(),
            dirty: false,
            last_save: Instant::now(),
            query_text: q,
            sort,
            shown: Vec::new(),
            pos_of: HashMap::new(),
            shown_gen: 0,
            sel: HashSet::new(),
            focus: None,
            anchor: None,
            view: View::Grid,
            back_view: View::Grid,
            cmp_ids: Vec::new(),
            cmp_focus: 0,
            zp: ZoomPan { zoom: 1.0, center: vec2(0.5, 0.5), actual: false },
            loader,
            tex: HashMap::new(),
            frame_no: 0,
            cols: 1,
            aspect: 0.7,
            row_h: 100.0,
            scroll_to: None,
            scroll_offset: 0.0,
            view_h: 100.0,
            force_scroll: None,
            scan_rx: None,
            scan_progress: None,
            scan_quiet: false,
            last_scan: Instant::now(),
            import_rx: None,
            import_progress: None,
            pending_known: HashMap::new(),
            import_queue: Vec::new(),
            import_album: None,
            pending_album: Vec::new(),
            export_rx: None,
            export_progress: None,
            export_tag: String::new(),
            notices: VecDeque::new(),
            ctl_rx,
            ctl_port,
            inject: VecDeque::new(),
            inject_replies: Vec::new(),
            inject_ptr: Pos2::new(10.0, 10.0),
            inject_drop: Vec::new(),
            layout: HashMap::new(),
            tag_input: String::new(),
            focus_tag_input: false,
            focus_search: false,
            modal: None,
            tag_counts: Vec::new(),
            album_counts: HashMap::new(),
            album_tree_counts: HashMap::new(),
            kind_counts: (0, 0),
            album_sig: Vec::new(),
            unfiled_n: 0,
            album_folded: HashSet::new(),
            album_clip: None,
            pic_drag: None,
            album_drop: Vec::new(),
            album_sel: HashSet::new(),
            album_anchor: None,
            album_focus: None,
            album_click_open_t: -10.0,
            scroll_to_cell: None,
            album_drag: Vec::new(),
            album_cover: HashMap::new(),
            prev_view: View::Grid,
            top_rest_w: 780.0,
            ckpt_counts: Vec::new(),
            lora_counts: Vec::new(),
            corner_drag: false,
            folder_counts: Vec::new(),
            score_counts: [0; 7],
            missing_n: 0,
            trash_n: 0,
            counts_dirty: true,
            undo: Vec::new(),
            ctx: cc.egui_ctx.clone(),
            window_size: vec2(settings.win_w, settings.win_h),
            window_pos: None,
            maximized: false,
            rand_seed: fastrand::u64(..),
            marquee: None,
            press: None,
            mx_cache: None,
            mx_hidden: HashSet::new(),
            mx_focus: None,
            mx_menu: false,
            watch_seen: ops::load_seen(),
            last_watch: Instant::now() - WATCH_EVERY + Duration::from_secs(5),
            fullscreen: false,
            offscreen: std::env::var_os("IMGR_OFFSCREEN").is_some(),
            focus_asked,
            focus_seen: 0,
            first_frame: true,
            mut_gen: 0,
            scan_gen: 0,
            save_tx,
            save_err,
            rehash,
            settings,
        };
        app.reindex();
        app.refilter(false);
        // come back to the picture that was open last time
        let last = app.settings.last_focus.clone();
        if let Some(&p) = app.pos_of.get(&last) {
            app.focus = Some(p);
            app.anchor = Some(p);
            app.sel.insert(last);
            app.scroll_to = Some(p);
        }
        if let Some(w) = db_warn {
            app.notice(w, true);
        }
        if let Some(n) = app.db.moved_from_json {
            app.notice(trf!("기록을 db.json에서 data\\imgr.db로 옮겼습니다 ({n}장). 예전 판(v0.16 이하)은 이제 이 data를 열지 않습니다", "Moved the records from db.json into data\\imgr.db ({n} pictures). Older versions (v0.16 and before) no longer open this data"), false);
        }
        if app.db.images.is_empty() {
            app.db.version = db::DB_VERSION;
        }
        if !app.db.read_only && !app.db.presets_to_albums {
            // v0.04: presets gave way to albums; keep what they grouped, once
            let n = app.db.presets_into_albums();
            if n > 0 {
                app.notice(trf!("프리셋 {n}개를 앨범 '프리셋' 아래 같은 이름의 앨범으로 옮겼습니다", "Moved {n} presets into same-named albums under the album '프리셋'"), false);
            }
            app.dirty = true;
            app.counts_dirty = true;
        }
        app.start_scan(false);
        app
    }

    // ------------------------------------------------------------ helpers
    fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.notices.push_back(Notice { text: text.into(), error, at: Instant::now() });
        while self.notices.len() > 6 {
            self.notices.pop_front();
        }
    }

    fn reindex(&mut self) {
        self.id_index = self.db.images.iter().enumerate().map(|(i, im)| (im.id.clone(), i)).collect();
    }

    fn img_path(&self, i: &Img) -> PathBuf {
        db::src_dir().join(i.path.replace('/', "\\"))
    }

    fn img(&self, id: &str) -> Option<&Img> {
        self.id_index.get(id).map(|&i| &self.db.images[i])
    }

    fn focus_img(&self) -> Option<&Img> {
        self.focus.and_then(|f| self.shown.get(f)).map(|&i| &self.db.images[i])
    }

    /// Selected pictures in display order (falls back to the focused one).
    /// In compare view: only the focused cell.
    fn selected_ids(&self) -> Vec<String> {
        if self.view == View::Compare {
            if let Some(id) = self.cmp_ids.get(self.cmp_focus) {
                return vec![id.clone()];
            }
        }
        if self.view == View::Viewer {
            return self.focus_img().map(|im| vec![im.id.clone()]).unwrap_or_default();
        }
        let mut v: Vec<String> = Vec::new();
        if !self.sel.is_empty() {
            v = self.shown.iter().map(|&i| &self.db.images[i]).filter(|im| self.sel.contains(&im.id)).map(|im| im.id.clone()).collect();
            // selected but not shown any more (e.g. after a search change): keep them last
            for id in &self.sel {
                if !self.pos_of.contains_key(id) && self.id_index.contains_key(id) {
                    v.push(id.clone());
                }
            }
        }
        if v.is_empty() {
            if let Some(im) = self.focus_img() {
                v.push(im.id.clone());
            }
        }
        v
    }

    fn paths_of(&self, ids: &[String]) -> Vec<PathBuf> {
        ids.iter().filter_map(|id| self.img(id)).filter(|i| !i.missing).map(|i| self.img_path(i)).collect()
    }

    /// How a search names an album: its name when no other album has it, else
    /// its path (trip/test), else (odd names) its id.
    pub(super) fn album_ref(&self, id: u32) -> String {
        let Some(a) = self.db.album(id) else { return id.to_string() };
        let ok = |s: &str| {
            let l = s.to_lowercase();
            !l.is_empty()
                && l.parse::<u32>().is_err()
                && !["top", "맨위", "none", "없음", "무소속"].contains(&l.as_str())
                && !s.contains(['|', '"'])
                && !s.ends_with('+')
                && !s.starts_with('-')
        };
        if ok(&a.name) && self.db.albums.iter().filter(|b| b.name.eq_ignore_ascii_case(&a.name)).count() == 1 {
            return a.name.clone();
        }
        // a path names one album (a path wins over a name: see query::set_album_names)
        let path = self.album_slash_path(id);
        let lp = path.to_lowercase();
        if ok(&path) && self.db.albums.iter().filter(|b| self.album_slash_path(b.id).to_lowercase() == lp).count() == 1 {
            return path;
        }
        id.to_string()
    }

    fn album_slash_path(&self, id: u32) -> String {
        self.db.album_chain(id).iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join("/")
    }

    /// The search text for an album in the album view (`Some(0)` = the top, `None` = 무소속).
    pub(super) fn album_q(&self, a: Option<u32>) -> String {
        let name = match a {
            Some(id) if id != 0 => self.album_ref(id),
            _ => String::new(),
        };
        query::album_query(a, &name)
    }

    /// Tell the search the album names (album:trip+) when the albums changed, and
    /// rewrite the album words of the current search to the new names first.
    fn sync_album_names(&mut self) {
        let sig: Vec<(u32, String, u32)> = self.db.albums.iter().map(|a| (a.id, a.name.clone(), a.parent)).collect();
        if sig == self.album_sig {
            return;
        }
        // resolved with the old names (still known to the search), written with the new ones
        let q = query::rename_album_words(&self.query_text, |id| self.album_ref(id));
        let mut keys = Vec::new();
        for a in &self.db.albums {
            keys.push((a.name.to_lowercase(), a.id, false));
            keys.push((self.album_slash_path(a.id).to_lowercase(), a.id, true));
        }
        query::set_album_names(keys);
        self.album_sig = sig;
        if q != self.query_text {
            self.query_text = q.clone();
            self.settings.query = q;
        }
    }

    fn refilter(&mut self, keep_focus: bool) {
        self.sync_album_names();
        let mut q = Query::parse(&self.query_text);
        for t in q.album_trees() {
            let sub = self.db.album_subtree(t);
            q.trees.insert(t, sub);
        }
        let sort = q.sort.unwrap_or(self.sort);
        let focus_id = if keep_focus { self.focus_img().map(|i| i.id.clone()) } else { None };
        let anchor_id = self.anchor.and_then(|a| self.shown.get(a)).map(|&i| self.db.images[i].id.clone());
        // 태그 여부 (워크플로 / 태그없음) filters searches; an album opened in the
        // album view (album:<name>, album:top) shows all its pictures
        let (wf, plain) = (self.settings.kind_wf, self.settings.kind_plain);
        // and so does the trash (a place too)
        let kind_on = !(wf && plain) && !matches!(query::album_word(&self.query_text), Some(Some(_))) && !query::has_word(&self.query_text, "is:trash");
        let mut v: Vec<usize> =
            (0..self.db.images.len()).filter(|&i| q.matches(&self.db.images[i]) && (!kind_on || self.db.images[i].has_workflow() == wf)).collect();
        let im = &self.db.images;
        match sort {
            Sort::New => v.sort_by(|&a, &b| (im[b].added, im[b].mtime, &im[b].path).cmp(&(im[a].added, im[a].mtime, &im[a].path))),
            Sort::Old => v.sort_by(|&a, &b| (im[a].added, im[a].mtime, &im[a].path).cmp(&(im[b].added, im[b].mtime, &im[b].path))),
            Sort::Name => v.sort_by(|&a, &b| im[a].name().to_lowercase().cmp(&im[b].name().to_lowercase())),
            Sort::Score => v.sort_by(|&a, &b| im[b].score.cmp(&im[a].score).then(im[b].mtime.cmp(&im[a].mtime))),
            Sort::Seed => v.sort_by(|&a, &b| (im[a].seed, &im[a].preset, im[a].name()).cmp(&(im[b].seed, &im[b].preset, im[b].name()))),
            Sort::Size => v.sort_by(|&a, &b| im[b].size.cmp(&im[a].size)),
            Sort::Random => {
                let s = self.rand_seed;
                v.sort_by_key(|&a| mix(&im[a].id, s));
            }
        }
        // usual width/height of the shown pictures (median of a sample) -> grid cell shape
        let mut ar: Vec<f32> = v.iter().take(400).map(|&i| &im[i]).filter(|x| x.w > 0 && x.h > 0).map(|x| x.w as f32 / x.h as f32).collect();
        if !ar.is_empty() {
            ar.sort_by(|a, b| a.total_cmp(b));
            self.aspect = ar[ar.len() / 2].clamp(0.5, 1.6);
        }
        self.shown = v;
        self.shown_gen += 1;
        self.pos_of = self.shown.iter().enumerate().map(|(p, &i)| (self.db.images[i].id.clone(), p)).collect();
        self.focus = focus_id.and_then(|id| self.pos_of.get(&id).copied()).or(if self.shown.is_empty() { None } else { Some(0) });
        if !keep_focus {
            self.scroll_to = Some(0);
            self.sel.clear();
            self.anchor = self.focus;
        } else {
            let keep: HashSet<String> = self.sel.iter().filter(|id| self.pos_of.contains_key(*id)).cloned().collect();
            self.sel = keep;
            self.anchor = anchor_id.and_then(|id| self.pos_of.get(&id).copied()).or(self.focus);
        }
        for id in self.loader.clear_thumbs() {
            if let Some(Slot::Pending(_)) = self.tex.get(&(id.clone(), Kind::Thumb)) {
                self.tex.remove(&(id, Kind::Thumb));
            }
        }
    }

    fn set_query(&mut self, q: String) {
        self.query_text = q;
        self.refilter(false);
        self.settings.query = self.query_text.clone();
        self.settings.push_search(&self.query_text.clone());
        if matches!(self.view, View::Viewer | View::Compare) {
            self.view = if self.back_view == View::Albums { View::Albums } else { View::Grid };
        }
    }

    fn recount(&mut self) {
        self.tag_counts = self.db.all_tags();
        self.album_counts.clear();
        self.album_tree_counts.clear();
        self.unfiled_n = 0;
        let parent: HashMap<u32, u32> = self.db.albums.iter().map(|a| (a.id, a.parent)).collect();
        let mut up: HashSet<u32> = HashSet::new();
        for i in self.db.images.iter().filter(|i| !i.missing && !i.trash) {
            if i.albums.is_empty() {
                self.unfiled_n += 1;
            }
            up.clear();
            for a in &i.albums {
                *self.album_counts.entry(*a).or_default() += 1;
                // the album and every album above it (a picture counts once per album)
                let mut x = *a;
                let mut guard = 0;
                while x != 0 && guard < 64 && up.insert(x) {
                    x = parent.get(&x).copied().unwrap_or(0);
                    guard += 1;
                }
            }
            for a in &up {
                *self.album_tree_counts.entry(*a).or_default() += 1;
            }
        }
        self.ckpt_counts = self.db.counts_by(|i| &i.ckpt);
        {
            let mut m: HashMap<String, usize> = HashMap::new();
            for i in self.db.images.iter().filter(|i| !i.missing && !i.trash) {
                for l in &i.loras {
                    if !l.is_empty() {
                        *m.entry(l.clone()).or_default() += 1;
                    }
                }
            }
            let mut v: Vec<(String, usize)> = m.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            self.lora_counts = v;
        }
        self.folder_counts = self.db.counts_by(|i| i.folder());
        self.folder_counts.sort();
        self.score_counts = [0; 7];
        self.kind_counts = (0, 0);
        self.missing_n = 0;
        self.trash_n = 0;
        for i in &self.db.images {
            if i.missing {
                self.missing_n += 1;
                continue;
            }
            if i.trash {
                self.trash_n += 1;
                continue;
            }
            self.score_counts[i.score.min(6) as usize] += 1;
            if i.has_workflow() {
                self.kind_counts.0 += 1;
            } else {
                self.kind_counts.1 += 1;
            }
        }
        self.counts_dirty = false;
    }

    fn mark_changed(&mut self) {
        self.dirty = true;
        self.counts_dirty = true;
    }

    fn save_now(&mut self) {
        match self.db.take_delta() {
            Ok(b) => {
                if !b.is_empty() {
                    let _ = self.save_tx.send((b, None));
                }
                self.dirty = false;
            }
            Err(e) => {
                if self.last_save.elapsed() > Duration::from_secs(60) || self.dirty {
                    self.notice(trf!("저장 안 함: {e}", "Not saved: {e}"), true);
                }
                self.dirty = false;
            }
        }
        self.last_save = Instant::now();
        self.settings.save();
    }

    /// Save and wait until it is on disk (on exit).
    fn save_blocking(&mut self) {
        if let Ok(b) = self.db.take_delta() {
            let (atx, arx) = channel();
            if self.save_tx.send((b, Some(atx))).is_ok() {
                let _ = arx.recv_timeout(Duration::from_secs(20));
            }
        }
        self.settings.save();
    }

    // ------------------------------------------------------------ background jobs
    fn start_scan(&mut self, quiet: bool) {
        if self.scan_rx.is_some() {
            return;
        }
        let mut known: db::Known = if self.rehash {
            // ids from an older version: hash every file again (records follow by path)
            HashMap::new()
        } else {
            self.db.images.iter().map(|i| (i.path.clone(), (i.id.clone(), i.size, i.mtime, i.meta_read))).collect()
        };
        for (k, v) in self.pending_known.drain() {
            known.entry(k).or_insert(v);
        }
        self.scan_gen = self.mut_gen;
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        let had_live = self.db.images.iter().any(|i| !i.missing);
        std::thread::spawn(move || {
            let src = db::src_dir();
            // the picture folder itself is gone (drive, renamed folder): never read that as "all deleted"
            let lost = had_live && !src.is_dir();
            let _ = std::fs::create_dir_all(&src);
            let ptx = tx.clone();
            let pctx = ctx.clone();
            let (items, errors) = db::scan(&src, &known, &move |d, t| {
                let _ = ptx.send(ScanMsg::Progress(d, t));
                pctx.request_repaint();
            });
            let _ = tx.send(ScanMsg::Done(items, errors + lost as usize));
            ctx.request_repaint();
        });
        self.scan_rx = Some(rx);
        self.scan_quiet = quiet;
        if !quiet {
            self.scan_progress = Some((0, 0));
        }
        self.last_scan = Instant::now();
    }

    fn start_import(&mut self, paths: Vec<PathBuf>, mv: bool) {
        self.start_import_to(paths, mv, None);
    }

    /// Import; `album` = put the pictures (new ones and ones already in) into that album too.
    fn start_import_to(&mut self, paths: Vec<PathBuf>, mv: bool, album: Option<Option<u32>>) {
        if self.import_rx.is_some() {
            // e.g. a second drop while the first is still copying: do it right after
            self.import_queue.push((paths, mv, album));
            self.notice(tr("가져오기가 진행 중이라, 끝나면 이어서 넣습니다", "An import is running; this will be added when it finishes"), false);
            return;
        }
        // pictures just put in but not scanned yet count as known too
        let ids: HashSet<String> =
            self.db.images.iter().filter(|i| !i.missing).map(|i| i.id.clone()).chain(self.pending_known.values().map(|v| v.0.clone())).collect();
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let ptx = tx.clone();
            let pctx = ctx.clone();
            let rep = db::import(&paths, &ids, mv, &move |d, t| {
                let _ = ptx.send(ImportMsg::Progress(d, t));
                pctx.request_repaint();
            });
            let _ = tx.send(ImportMsg::Done(rep, Vec::new(), None));
            ctx.request_repaint();
        });
        self.import_rx = Some(rx);
        self.import_progress = Some((0, 0));
        self.import_album = album;
    }

    /// Import new files from the watch folders (in the background).
    fn check_watch(&mut self) {
        self.last_watch = Instant::now();
        if self.settings.watch_dirs.is_empty() || self.import_rx.is_some() {
            return;
        }
        let dirs = self.settings.watch_dirs.clone();
        let seen = self.watch_seen.clone();
        let mv = self.settings.watch_move;
        let ids: HashSet<String> = self.db.images.iter().filter(|i| !i.missing).map(|i| i.id.clone()).collect();
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let (new, alive) = ops::watch_new(&dirs, &seen);
            let paths: Vec<PathBuf> = new.iter().map(|x| x.0.clone()).collect();
            let rep = if paths.is_empty() {
                db::ImportReport::default()
            } else {
                let ptx = tx.clone();
                let pctx = ctx.clone();
                db::import(&paths, &ids, mv, &move |d, t| {
                    let _ = ptx.send(ImportMsg::Progress(d, t));
                    pctx.request_repaint();
                })
            };
            // failed files are not marked seen, so they are tried again next time
            let failed: HashSet<String> = rep.failed.iter().filter_map(|f| f.rsplit_once(": ").map(|(p, _)| p.to_string())).collect();
            let marks: Vec<(String, (u64, u64))> =
                new.into_iter().filter(|(p, ..)| !failed.contains(&p.display().to_string())).map(|(_, k, s, m)| (k, (s, m))).collect();
            let _ = tx.send(ImportMsg::Done(rep, marks, Some(alive)));
            ctx.request_repaint();
        });
        self.import_rx = Some(rx);
    }

    fn start_export(&mut self, ids: Vec<String>, dir: PathBuf, fmt: ops::Fmt, upload: bool, tag: String) {
        if self.export_rx.is_some() {
            self.notice(tr("내보내기가 이미 진행 중입니다", "An export is already running"), true);
            return;
        }
        let files: Vec<(String, PathBuf)> = ids.iter().filter_map(|id| self.img(id)).filter(|i| !i.missing).map(|i| (i.id.clone(), self.img_path(i))).collect();
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let ptx = tx.clone();
            let pctx = ctx.clone();
            let rep = ops::export(&files, &dir, fmt, upload, &move |d, t| {
                let _ = ptx.send(ExportMsg::Progress(d, t));
                pctx.request_repaint();
            });
            let _ = tx.send(ExportMsg::Done(rep));
            ctx.request_repaint();
        });
        self.export_rx = Some(rx);
        self.export_progress = Some((0, ids.len()));
        self.export_tag = tag;
    }

    fn poll_background(&mut self) {
        // loaded images -> textures
        while let Ok(l) = self.loader.rx.try_recv() {
            let key = (l.id.clone(), l.kind);
            if !matches!(self.tex.get(&key), Some(Slot::Pending(_))) {
                continue; // no longer wanted
            }
            match l.image {
                Some(img) => {
                    let h = self.ctx.load_texture(format!("{}{:?}", l.id, l.kind), img, egui::TextureOptions::LINEAR);
                    self.tex.insert(key, Slot::Ready(h, self.frame_no));
                }
                None => {
                    self.tex.insert(key, Slot::Failed(Instant::now()));
                }
            }
        }
        self.evict_textures();
        // scan
        let mut done = None;
        if let Some(rx) = &self.scan_rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    ScanMsg::Progress(d, t) => {
                        if !self.scan_quiet {
                            self.scan_progress = Some((d, t))
                        }
                    }
                    ScanMsg::Done(items, errors) => done = Some((items, errors)),
                }
            }
        }
        if let Some((items, errors)) = done {
            self.scan_rx = None;
            self.scan_progress = None;
            if self.scan_gen != self.mut_gen {
                // files were renamed / removed while this scan ran: its list is stale
                let quiet = self.scan_quiet;
                self.start_scan(quiet);
                return;
            }
            let meta_new = items.iter().any(|i| i.meta.is_some());
            let before = self.db.images.len();
            let keep = self.focus_ids();
            // the start-up scan and F5 / ⟳ are asked for: believe the folder even
            // when many pictures vanished at once (only read errors hold it)
            let sure = !self.scan_quiet;
            let db::MergeReport { new, moved, gone, held, forgot } = self.db.merge_scan(items, errors, sure);
            if self.rehash {
                self.rehash = false;
                self.db.version = db::DB_VERSION;
                self.dirty = true;
            }
            if held {
                let why = if errors > 0 { trf!("읽기 오류 {errors}", "{errors} read errors") } else { tr("한꺼번에", "all at once").to_string() };
                self.notice(trf!("imgs에서 {gone}장이 안 보입니다 ({why}). 드라이브·폴더 문제일 수 있어 '사라짐' 표시를 미뤘습니다. 정말 지운 거면 F5", "{gone} pictures are missing from imgs ({why}). It may be a drive or folder problem, so marking them as missing was postponed. If you really deleted them, press F5"), true);
            }
            let gone = if held { 0 } else { gone };
            if new + moved + gone > 0 || meta_new || before != self.db.images.len() {
                if new + moved + gone > 0 {
                    self.notice(trf!("새 그림 {new}, 옮겨지거나 이름 바뀐 것 {moved}, 사라진 것 {gone}", "New pictures {new}, moved or renamed {moved}, gone {gone}"), false);
                }
                // drop textures of pictures that are gone, so no stale picture is drawn
                let live: HashSet<&str> = self.db.images.iter().filter(|i| !i.missing).map(|i| i.id.as_str()).collect();
                self.tex.retain(|k, _| live.contains(k.0.as_str()));
                self.reindex();
                self.apply_pending_album();
                self.mark_changed();
                self.save_now();
                if forgot > 0 {
                    self.refilter_after_removal(keep);
                } else {
                    self.refilter(true);
                }
                self.mx_cache = None;
            }
            if !self.scan_quiet || gone + forgot > 0 {
                // thumbnails of pictures whose file is gone are removed too
                // (a returning file simply gets a new one)
                let keep: HashSet<String> = self.db.images.iter().filter(|i| !i.missing).map(|i| i.id.clone()).collect();
                let dir = db::data_dir().join("thumbs");
                std::thread::spawn(move || thumbs::prune(&dir, &keep));
            }
        }
        let mut idone = None;
        if let Some(rx) = &self.import_rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    ImportMsg::Progress(d, t) => self.import_progress = Some((d, t)),
                    ImportMsg::Done(r, marks, alive) => idone = Some((r, marks, alive)),
                }
            }
        }
        if let Some((r, marks, alive)) = idone {
            self.import_rx = None;
            self.import_progress = None;
            let watch = alive.is_some();
            let before = self.watch_seen.len();
            let changed = !marks.is_empty();
            for (k, v) in marks {
                self.watch_seen.insert(k, v);
            }
            if let Some(alive) = alive {
                self.watch_seen.retain(|k, _| alive.contains(k));
            }
            if changed || before != self.watch_seen.len() {
                ops::save_seen(&self.watch_seen);
            }
            if r.copied > 0 || !r.failed.is_empty() || !watch {
                let mut msg = trf!("{}가져오기: {}장 넣음, 같은 그림 {}장 건너뜀", "{}Import: {} pictures added, {} duplicates skipped", if watch { tr("자동 ", "Auto ") } else { "" }, r.copied, r.skipped_dup);
                if r.renamed > 0 {
                    msg.push_str(&trf!(", 이름이 겹쳐 '이름 (2)'처럼 바꾼 것 {}장", ", {} renamed like 'name (2)' because of name clashes", r.renamed));
                }
                if !r.failed.is_empty() {
                    msg.push_str(&trf!(", 실패 {}장 ({})", ", {} failed ({})", r.failed.len(), r.failed[0]));
                }
                let err = !r.failed.is_empty();
                self.notice(msg, err);
            }
            for (path, id, size, mtime) in r.added.iter().cloned() {
                self.pending_known.insert(path, (id, size, mtime, false));
            }
            if let Some(Some(a)) = self.import_album.take() {
                // new pictures get their record at the next scan: remember them till then
                let ids: Vec<String> = r.added.iter().map(|x| x.1.clone()).chain(r.dup_ids.iter().cloned()).collect();
                self.pending_album.extend(ids.into_iter().map(|id| (id, a)));
                self.apply_pending_album();
            }
            if r.copied > 0 {
                // a scan already running started before these files: let it run again
                self.mut_gen += 1;
                self.start_scan(false);
            }
            if !self.import_queue.is_empty() {
                // next batch: same move flag and album as the first waiting one
                let q = std::mem::take(&mut self.import_queue);
                let (mv, al) = (q[0].1, q[0].2);
                let (same, rest): (Vec<_>, Vec<_>) = q.into_iter().partition(|x| x.1 == mv && x.2 == al);
                self.import_queue = rest;
                self.start_import_to(same.into_iter().flat_map(|x| x.0).collect(), mv, al);
            }
        }
        let mut edone = None;
        if let Some(rx) = &self.export_rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    ExportMsg::Progress(d, t) => self.export_progress = Some((d, t)),
                    ExportMsg::Done(r) => edone = Some(r),
                }
            }
        }
        if let Some(r) = edone {
            self.export_rx = None;
            self.export_progress = None;
            let tag = clean_tag(&self.export_tag);
            if !tag.is_empty() && !r.ids.is_empty() {
                self.edit(&r.ids, &trf!("#{tag} 붙이기", "Add #{tag}"), |im| {
                    if !im.tags.contains(&tag) {
                        im.tags.push(tag.clone());
                    }
                });
                self.settings.push_recent(&tag);
            }
            let mut msg = trf!("{}장 내보냄 → {}", "{} pictures exported → {}", r.done, r.out_dir.display());
            if !tag.is_empty() {
                msg.push_str(&trf!("  (#{tag} 붙임)", "  (#{tag} added)"));
            }
            if !r.failed.is_empty() {
                msg.push_str(&trf!(", 실패 {} ({})", ", {} failed ({})", r.failed.len(), r.failed[0]));
            }
            let err = !r.failed.is_empty();
            self.notice(msg, err);
            if r.done > 0 && !self.offscreen {
                let _ = std::process::Command::new("explorer").arg(&r.out_dir).spawn();
            }
        }
        if self.dirty && self.last_save.elapsed() > Duration::from_secs(3) {
            self.save_now();
        }
        let werr = self.save_err.lock().map(|mut e| e.take()).unwrap_or(None);
        if let Some(e) = werr {
            self.notice(trf!("imgr.db 저장 실패: {e}", "Could not save imgr.db: {e}"), true);
            self.db.forget_saved();
            self.dirty = true;
        }
        // periodic quiet rescan (pictures put into imgs with Explorer) and watch folders
        if self.scan_rx.is_none() && self.import_rx.is_none() && self.last_scan.elapsed() > RESCAN_EVERY {
            self.start_scan(true);
        }
        if !self.settings.watch_dirs.is_empty() && self.import_rx.is_none() && self.scan_rx.is_none() && self.last_watch.elapsed() > WATCH_EVERY {
            self.check_watch();
        }
        let now = Instant::now();
        self.notices.retain(|n| now.duration_since(n.at) < Duration::from_secs(if n.error { 25 } else { 9 }));
    }

    fn evict_textures(&mut self) {
        if self.frame_no % 120 == 0 {
            self.tex.retain(|_, s| !matches!(s, Slot::Pending(t) if t.elapsed() > Duration::from_secs(30)));
        }
        let thumbs = self.tex.keys().filter(|k| k.1 == Kind::Thumb).count();
        let bigs = self.tex.len() - thumbs;
        if thumbs <= MAX_THUMBS && bigs <= MAX_BIGS {
            return;
        }
        let mut v: Vec<((String, Kind), u64)> = self
            .tex
            .iter()
            .filter_map(|(k, s)| match s {
                Slot::Ready(_, u) => Some((k.clone(), *u)),
                Slot::Failed(_) => Some((k.clone(), 0)),
                Slot::Pending(_) => None,
            })
            .collect();
        v.sort_by_key(|x| x.1);
        let (mut t, mut b) = (thumbs, bigs);
        for (k, used) in v {
            if used + 2 >= self.frame_no {
                continue;
            }
            if k.1 == Kind::Thumb && t > MAX_THUMBS * 9 / 10 {
                self.tex.remove(&k);
                t -= 1;
            } else if k.1 != Kind::Thumb && b > MAX_BIGS - 4 {
                self.tex.remove(&k);
                b -= 1;
            }
        }
    }

    /// Texture for a picture, requesting it if needed.
    fn texture(&mut self, id: &str, path: PathBuf, kind: Kind) -> Option<egui::TextureHandle> {
        let key = (id.to_string(), kind);
        match self.tex.get_mut(&key) {
            Some(Slot::Ready(h, used)) => {
                *used = self.frame_no;
                Some(h.clone())
            }
            Some(Slot::Pending(since)) => {
                if since.elapsed() > Duration::from_secs(8) {
                    *since = Instant::now();
                    self.loader.request(id, path, kind);
                }
                None
            }
            Some(Slot::Failed(at)) => {
                if at.elapsed() > Duration::from_secs(30) {
                    self.tex.insert(key, Slot::Pending(Instant::now()));
                    self.loader.request(id, path, kind);
                }
                None
            }
            None => {
                self.tex.insert(key, Slot::Pending(Instant::now()));
                self.loader.request(id, path, kind);
                None
            }
        }
    }

    /// Big texture if ready, else the thumbnail (while the big one loads).
    fn big_or_thumb(&mut self, id: &str, px: u32) -> Option<egui::TextureHandle> {
        let path = self.img(id).map(|i| self.img_path(i))?;
        self.texture(id, path.clone(), Kind::Big(px)).or_else(|| self.texture(id, path, Kind::Thumb))
    }

    fn thumb(&mut self, id: &str) -> Option<egui::TextureHandle> {
        let path = self.img(id).map(|i| self.img_path(i))?;
        self.texture(id, path, Kind::Thumb)
    }

    // ------------------------------------------------------------ editing
    fn edit<F: FnMut(&mut Img)>(&mut self, ids: &[String], label: &str, mut f: F) {
        let mut step = UndoStep { label: label.to_string(), items: Vec::new() };
        let idx: Vec<usize> = ids.iter().filter_map(|id| self.id_index.get(id).copied()).collect();
        for i in idx {
            let im = &mut self.db.images[i];
            step.items.push(UndoItem { id: im.id.clone(), score: im.score, tags: im.tags.clone(), trash: im.trash, preset: im.preset.clone(), preset_set: im.preset_set, albums: im.albums.clone() });
            f(im);
        }
        if !step.items.is_empty() {
            self.undo.push(step);
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.mark_changed();
            self.mx_cache = None;
        }
    }

    fn set_score(&mut self, n: u8) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        self.edit(&ids, &trf!("점수 {n}", "Score {n}"), |im| im.score = n);
        let msg = if n == 0 { trf!("{}장 점수 지움", "Score cleared on {} pictures", ids.len()) } else { trf!("{}장 → {n}점", "{} pictures → score {n}", ids.len()) };
        self.notice(msg, false);
        // rating one by one in the big viewer: go on to the next picture
        if self.view == View::Viewer && self.settings.auto_advance && n > 0 && ids.len() == 1 {
            if let Some(f) = self.focus {
                if f + 1 < self.shown.len() {
                    self.move_focus(1, false);
                }
            }
        }
    }

    fn add_tag(&mut self, raw: &str) {
        let t = clean_tag(raw);
        if t.is_empty() {
            return;
        }
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        self.edit(&ids, &trf!("#{t} 붙이기", "Add #{t}"), |im| {
            if !im.tags.contains(&t) {
                im.tags.push(t.clone());
            }
        });
        self.settings.push_recent(&t);
        self.notice(trf!("{}장에 #{t}", "#{t} on {} pictures", ids.len()), false);
    }

    fn remove_tag(&mut self, t: &str) {
        let ids = self.selected_ids();
        let t = t.to_string();
        self.edit(&ids, &trf!("#{t} 떼기", "Remove #{t}"), |im| im.tags.retain(|x| *x != t));
    }

    /// Chip click: if every selected picture has it, remove; else add to all.
    fn toggle_tag(&mut self, t: &str) {
        let ids: HashSet<String> = self.selected_ids().into_iter().collect();
        if ids.is_empty() {
            return;
        }
        let all = ids.iter().filter_map(|id| self.img(id)).all(|i| i.tags.iter().any(|x| x == t));
        if all {
            self.remove_tag(t);
        } else {
            self.add_tag(t);
        }
    }

    /// Rename (or merge into an existing) hashtag on every picture.
    fn rename_tag_all(&mut self, old: &str, new: &str) {
        let new = clean_tag(new);
        if new.is_empty() || new == old {
            return;
        }
        let ids: Vec<String> = self.db.images.iter().filter(|i| i.tags.iter().any(|t| t == old)).map(|i| i.id.clone()).collect();
        let o = old.to_string();
        self.edit(&ids, &format!("#{old} → #{new}"), |im| {
            im.tags.retain(|t| *t != o);
            if !im.tags.contains(&new) {
                im.tags.push(new.clone());
            }
        });
        for t in self.settings.recent_tags.iter_mut() {
            if t == old {
                *t = new.clone();
            }
        }
        let mut seen = HashSet::new();
        self.settings.recent_tags.retain(|t| seen.insert(t.clone()));
        let (wo, wn) = (format!("#{old}"), format!("#{new}"));
        let (nwo, nwn) = (format!("-#{old}"), format!("-#{new}"));
        for f in self.db.saved.iter_mut() {
            f.query = f
                .query
                .split(' ')
                .map(|w| if w == wo { wn.clone() } else if w == nwo { nwn.clone() } else { w.to_string() })
                .collect::<Vec<_>>()
                .join(" ");
        }
        self.notice(trf!("#{old} → #{new} ({}장, Ctrl+Z 되돌리기)", "#{old} → #{new} ({} pictures, Ctrl+Z to undo)", ids.len()), false);
        if query::has_word(&self.query_text, &wo) {
            let q = query::toggle_word(&self.query_text, &wo);
            self.set_query(query::toggle_word(&q, &wn));
        } else {
            self.refilter(true);
        }
    }

    fn delete_tag_all(&mut self, tag: &str) {
        let ids: Vec<String> = self.db.images.iter().filter(|i| i.tags.iter().any(|t| t == tag)).map(|i| i.id.clone()).collect();
        let t = tag.to_string();
        self.edit(&ids, &trf!("#{tag} 모두에서 떼기", "Remove #{tag} from all"), |im| im.tags.retain(|x| *x != t));
        self.settings.recent_tags.retain(|x| x != tag);
        self.notice(trf!("#{tag}를 {}장에서 뗌 (Ctrl+Z 되돌리기)", "Removed #{tag} from {} pictures (Ctrl+Z to undo)", ids.len()), false);
        self.refilter(true);
    }

    // ------------------------------------------------------------ albums
    /// Every album in tree order: (id, name, depth).
    fn album_rows(&self) -> Vec<(u32, String, usize)> {
        self.album_rows_open_inner(false).into_iter().map(|(a, b, c, _)| (a, b, c)).collect()
    }

    /// Albums of the side panel: folded ones hide their sub albums. (id, name, depth, has sub albums)
    fn album_rows_open(&self) -> Vec<(u32, String, usize, bool)> {
        self.album_rows_open_inner(true)
    }

    fn album_rows_open_inner(&self, fold: bool) -> Vec<(u32, String, usize, bool)> {
        let mut out = Vec::new();
        let mut stack: Vec<(u32, usize)> = self.db.album_children(0).iter().rev().map(|a| (a.id, 0)).collect();
        let mut seen = HashSet::new();
        while let Some((id, d)) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(a) = self.db.album(id) else { continue };
            let kids = self.db.album_children(id);
            out.push((id, a.name.clone(), d, !kids.is_empty()));
            if !(fold && self.album_folded.contains(&id)) {
                stack.extend(kids.iter().rev().map(|k| (k.id, d + 1)));
            }
        }
        out
    }

    /// Imported pictures waiting to go into an album: done once their record exists.
    fn apply_pending_album(&mut self) {
        if self.pending_album.is_empty() {
            return;
        }
        let mut left = Vec::new();
        let mut n = 0;
        for (id, a) in std::mem::take(&mut self.pending_album) {
            match self.id_index.get(&id).copied() {
                Some(i) => {
                    let alive = self.db.album(a).is_some();
                    let im = &mut self.db.images[i];
                    if alive && !im.albums.contains(&a) {
                        im.albums.push(a);
                        n += 1;
                    }
                }
                None => left.push((id, a)),
            }
        }
        // a picture whose file never got a record (copy failed...) is dropped after a while
        left.truncate(20000);
        self.pending_album = left;
        if n > 0 {
            self.mark_changed();
            self.refilter(true);
        }
    }

    /// The album the search shows (see `query::album_word`).
    /// (`None` = no album named, or the album top `album:top`)
    fn cur_album(&self) -> Option<Option<u32>> {
        query::album_word(&self.query_text).filter(|a| *a != Some(0))
    }

    /// Keep the album view and the search in step, without ever fighting what
    /// is typed: entering the album view with no album named starts at the top
    /// (`album:top`, the grid then shows only albums). Inside it, a search that
    /// names no album any more, or filters added at the top (the top has no
    /// pictures), is a search of all pictures: the view turns to the list.
    /// Other views drop `album:top`. Quietly: no search history entry.
    fn album_place_fix(&mut self) {
        let named = query::album_word(&self.query_text);
        let rest = query::without_album(&self.query_text);
        let entered = self.view == View::Albums && self.prev_view != View::Albums;
        let new_q = if self.view == View::Albums && !query::names_album(&self.query_text) {
            if entered {
                Some("album:top".to_string())
            } else {
                self.view = View::Grid;
                None
            }
        } else if self.view == View::Albums && named == Some(Some(0)) && !rest.trim().is_empty() {
            self.view = View::Grid;
            Some(rest)
        } else if matches!(self.view, View::Grid | View::Matrix) && named == Some(Some(0)) {
            Some(rest)
        } else {
            None
        };
        self.prev_view = self.view;
        if let Some(q) = new_q {
            self.query_text = q;
            self.refilter(false);
            self.settings.query = self.query_text.clone();
            self.album_sel.clear();
        }
    }

    /// Albums shown as folder cells in front of the pictures (album view only).
    pub(super) fn album_lead(&self) -> Vec<Option<u32>> {
        if self.view != View::Albums {
            return Vec::new();
        }
        match self.cur_album() {
            // the top: the top albums, then (as pictures) those in no album
            None => self.db.album_children(0).iter().map(|a| Some(a.id)).collect(),
            Some(Some(id)) => self.db.album_children(id).iter().map(|a| Some(a.id)).collect(),
            Some(None) => Vec::new(),
        }
    }

    /// Selected pictures, but none when only albums are selected (the
    /// focused picture is not meant then).
    pub(super) fn picked_pictures(&self) -> Vec<String> {
        if self.sel.is_empty() && !self.album_sel.is_empty() {
            Vec::new()
        } else {
            self.selected_ids()
        }
    }

    /// Click on an album cell: like a picture (Ctrl toggles, Shift a range).
    pub(super) fn click_album(&mut self, g: usize, lead: &[Option<u32>], m: Modifiers) {
        if m.shift {
            // a range over album cells and pictures alike (grid order)
            let a = self.cell_anchor(lead.len()).unwrap_or(g);
            self.select_cells(a, g, lead, m.ctrl);
            self.set_cell_cursor(g, lead.len(), false);
            return;
        }
        match lead.get(g).copied().flatten() {
            // 무소속 cannot be moved or deleted: a click only puts the cursor there
            None => {
                if !m.ctrl {
                    self.sel.clear();
                    self.album_sel.clear();
                }
            }
            Some(id) if m.ctrl => {
                if !self.album_sel.remove(&id) {
                    self.album_sel.insert(id);
                }
            }
            Some(id) => {
                self.sel.clear();
                self.album_sel.clear();
                self.album_sel.insert(id);
            }
        }
        self.set_cell_cursor(g, lead.len(), true);
    }

    /// Album view: start of a Shift range as a grid index (album cells first, then pictures).
    fn cell_anchor(&self, nl: usize) -> Option<usize> {
        self.album_anchor.filter(|&a| a < nl).or_else(|| self.anchor.map(|p| p + nl))
    }

    /// Album view: the keyboard cursor as a grid index.
    fn cell_cursor(&self, nl: usize) -> Option<usize> {
        self.album_focus.filter(|&a| a < nl).or_else(|| self.focus.map(|p| p + nl))
    }

    /// Album view: put the cursor (and, with `anchor`, the Shift range start) on grid index g.
    fn set_cell_cursor(&mut self, g: usize, nl: usize, anchor: bool) {
        if g < nl {
            self.album_focus = Some(g);
            self.focus = None;
            if anchor {
                self.album_anchor = Some(g);
                self.anchor = None;
            }
        } else {
            self.album_focus = None;
            self.focus = Some(g - nl);
            if anchor {
                self.anchor = Some(g - nl);
                self.album_anchor = None;
            }
        }
    }

    /// Album view: select grid indexes a..=b (album cells and pictures); `add` keeps what was selected.
    fn select_cells(&mut self, a: usize, b: usize, lead: &[Option<u32>], add: bool) {
        let nl = lead.len();
        if !add {
            self.sel.clear();
            self.album_sel.clear();
        }
        for g in a.min(b)..=a.max(b) {
            if g < nl {
                if let Some(id) = lead[g] {
                    self.album_sel.insert(id);
                }
            } else if let Some(&ix) = self.shown.get(g - nl) {
                self.sel.insert(self.db.images[ix].id.clone());
            }
        }
    }

    /// Album view: arrow keys, PageUp/Down, Home/End walk album cells and pictures as one grid.
    fn move_cell(&mut self, delta: isize, shift: bool) {
        let lead = self.album_lead();
        let nl = lead.len();
        let total = nl + self.shown.len();
        if total == 0 {
            return;
        }
        let cur = self.cell_cursor(nl).unwrap_or(0).min(total - 1);
        let n = (cur as isize).saturating_add(delta).clamp(0, total as isize - 1) as usize;
        if shift {
            let a = self.cell_anchor(nl).unwrap_or(cur).min(total - 1);
            self.select_cells(a, n, &lead, false);
            self.set_cell_cursor(a, nl, true);
            self.set_cell_cursor(n, nl, false);
        } else {
            self.select_cells(n, n, &lead, false);
            self.set_cell_cursor(n, nl, true);
        }
        self.scroll_to_cell = Some(n);
        if !self.zp.actual {
            self.zp.reset();
        }
    }

    /// "3장", "앨범 2개", "3장 · 앨범 2개"
    fn count_text(pics: usize, albums: usize) -> String {
        match (pics, albums) {
            (p, 0) => trf!("{p}장", "{p} pictures"),
            (0, a) => trf!("앨범 {a}개", "{a} albums"),
            (p, a) => trf!("{p}장 · 앨범 {a}개", "{p} pictures · {a} albums"),
        }
    }

    /// Put albums under another one (0 = the top).
    fn move_albums(&mut self, ids: &[u32], parent: u32) {
        let (mut ok, mut refused) = (0, 0);
        for &id in ids {
            if self.db.album(id).is_some_and(|a| a.parent == parent) {
                continue;
            }
            if id != parent && self.db.move_album(id, parent) {
                ok += 1;
            } else {
                refused += 1;
            }
        }
        if parent != 0 {
            self.album_folded.remove(&parent);
        }
        if ok > 0 {
            self.mark_changed();
        }
        let to = self.album_label(Some(parent));
        let mut msg = trf!("앨범 {ok}개를 '{to}'(으)로 옮김", "Moved {ok} albums into '{to}'");
        if refused > 0 {
            msg.push_str(&trf!(" — {refused}개는 자기 자신이나 그 안으로는 옮길 수 없어 그대로", " — {refused} left as they are (an album cannot move into itself)"));
        }
        self.notice(msg, ok == 0 && refused > 0);
        self.refilter(true);
    }

    fn album_label(&self, a: Option<u32>) -> String {
        match a {
            Some(0) => tr("맨 위", "Top").to_string(),
            Some(id) => self.db.album(id).map(|x| x.name.clone()).unwrap_or_else(|| trf!("앨범 {id}", "Album {id}")),
            None => tr("무소속", "No album").to_string(),
        }
    }

    /// Put pictures into an album (`None` = 앨범 없음: take them out of every album).
    fn add_to_album(&mut self, ids: &[String], to: Option<u32>) {
        if ids.is_empty() {
            return;
        }
        if to == Some(0) {
            self.notice(tr("그림은 맨 위에 둘 수 없습니다 — 앨범을 열고 넣으세요", "Pictures cannot go at the top — open an album and put them there"), true);
            return;
        }
        let name = self.album_label(to);
        let n = match to {
            Some(a) => {
                let todo: Vec<String> = ids.iter().filter(|id| self.img(id).is_some_and(|i| !i.albums.contains(&a))).cloned().collect();
                self.edit(&todo, &trf!("앨범 '{name}'에 넣기", "Add to album '{name}'"), |im| im.albums.push(a));
                todo.len()
            }
            None => {
                let todo: Vec<String> = ids.iter().filter(|id| self.img(id).is_some_and(|i| !i.albums.is_empty())).cloned().collect();
                self.edit(&todo, tr("앨범에서 빼기", "Remove from album"), |im| im.albums.clear());
                todo.len()
            }
        };
        let msg = match to {
            Some(_) if n < ids.len() => trf!("{n}장을 '{name}'에 넣음 ({}장은 이미 있음)", "Added {n} pictures to '{name}' ({} already there)", ids.len() - n),
            Some(_) => trf!("{n}장을 '{name}'에 넣음 (Ctrl+Z 되돌리기)", "Added {n} pictures to '{name}' (Ctrl+Z to undo)"),
            None => trf!("{n}장을 모든 앨범에서 뺌 (Ctrl+Z 되돌리기)", "Removed {n} pictures from all albums (Ctrl+Z to undo)"),
        };
        self.notice(msg, false);
        self.refilter(true);
    }

    /// Move pictures from one album to another (`from` None = from every album).
    fn move_to_album(&mut self, ids: &[String], from: Option<Option<u32>>, to: Option<u32>) {
        if ids.is_empty() {
            return;
        }
        let name = self.album_label(to);
        self.edit(ids, &trf!("앨범 '{name}'으로 옮기기", "Move to album '{name}'"), |im| {
            match from {
                Some(Some(f)) => im.albums.retain(|&a| a != f),
                _ => im.albums.clear(),
            }
            if let Some(t) = to {
                if !im.albums.contains(&t) {
                    im.albums.push(t);
                }
            }
        });
        self.notice(trf!("{}장을 '{name}'으로 옮김 (Ctrl+Z 되돌리기)", "Moved {} pictures to '{name}' (Ctrl+Z to undo)", ids.len()), false);
        self.refilter(true);
    }

    /// Ctrl+C (also copies the files for Explorer) / Ctrl+X: remember pictures for Ctrl+V into an album.
    fn album_copy(&mut self, cut: bool) {
        let ids = self.picked_pictures();
        let mut albums: Vec<u32> = if self.view == View::Albums { self.album_sel.iter().copied().collect() } else { Vec::new() };
        albums.sort_unstable();
        if ids.is_empty() && albums.is_empty() {
            return;
        }
        let from = self.cur_album();
        if cut {
            self.notice(trf!("{} 잘라 둠 — 옮길 앨범을 열고 Ctrl+V", "{} cut — open the target album and press Ctrl+V", Self::count_text(ids.len(), albums.len())), false);
        } else {
            if !ids.is_empty() {
                self.copy_files();
            }
            if !albums.is_empty() {
                // an album lives in one place: it is moved (Ctrl+X), not copied
                self.notice(tr("앨범은 한 곳에만 둘 수 있어 Ctrl+X → Ctrl+V로 옮깁니다 (Ctrl+C는 그림만)", "An album can be in one place only, so move it with Ctrl+X → Ctrl+V (Ctrl+C copies pictures only)"), ids.is_empty());
                albums.clear();
            }
        }
        if ids.is_empty() && albums.is_empty() {
            return;
        }
        self.album_clip = Some(AlbumClip { ids, cut, from, albums });
    }

    /// Ctrl+V: pictures copied / cut in imgr go into the album shown; files copied
    /// in Explorer are imported (and put into the album shown).
    fn paste_any(&mut self) {
        let cur = self.cur_album();
        let files = winshell::clipboard_files();
        let internal = match &self.album_clip {
            Some(c) if c.cut || !c.albums.is_empty() => true,
            // our own Ctrl+C put these very files on the clipboard
            Some(c) => !files.is_empty() && {
                let mine: HashSet<String> = self.paths_of(&c.ids).iter().map(|p| p.display().to_string().to_lowercase()).collect();
                files.iter().all(|f| mine.contains(&f.display().to_string().to_lowercase()))
            },
            None => false,
        };
        if !internal {
            if files.is_empty() {
                self.notice(tr("붙여 넣을 것이 없습니다 (그림을 고르고 Ctrl+C·Ctrl+X, 또는 탐색기에서 복사한 파일)", "Nothing to paste (select pictures and press Ctrl+C or Ctrl+X, or copy files in Explorer)"), true);
                return;
            }
            self.start_import_to(files, false, cur);
            return;
        }
        let clip = self.album_clip.take().unwrap();
        // albums go under the album shown (the top in the album view's top)
        if !clip.albums.is_empty() {
            let parent = match cur {
                Some(Some(p)) => Some(p),
                None if self.view == View::Albums => Some(0),
                _ => None,
            };
            let Some(parent) = parent else {
                self.notice(tr("앨범을 붙여 넣을 곳을 여세요 (🗂 앨범 화면에서 맨 위나 다른 앨범)", "Open where to paste the album (Top or another album in the 🗂 Album view)"), true);
                self.album_clip = Some(clip);
                return;
            };
            self.move_albums(&clip.albums, parent);
            if clip.ids.is_empty() {
                return;
            }
        }
        let Some(to) = cur else {
            if self.view == View::Albums && clip.cut {
                // pictures cut from an album, pasted at the top: they leave that album
                match clip.from {
                    Some(Some(f)) => self.out_to_top(&clip.ids, f),
                    _ => self.notice(tr("이미 맨 위에 있는 그림입니다", "That picture is already at the top"), false),
                }
            } else if self.view == View::Albums {
                self.notice(tr("맨 위에는 복사해 둘 수 없습니다 (Ctrl+X로 잘라서 붙이면 앨범에서 빠짐)", "Cannot copy to the top (cut with Ctrl+X and paste to take it out of the album)"), true);
                self.album_clip = Some(clip);
            } else if clip.albums.is_empty() {
                self.notice(tr("붙여 넣을 앨범을 먼저 여세요 (왼쪽 앨범 목록이나 🗂 앨범 화면)", "Open the album to paste into first (left album list or the 🗂 Album view)"), true);
                self.album_clip = Some(clip);
            }
            return;
        };
        if clip.cut {
            if clip.from == Some(to) {
                self.notice(tr("같은 앨범입니다", "Same album"), false);
            } else {
                self.move_to_album(&clip.ids, clip.from, to);
            }
        } else {
            self.add_to_album(&clip.ids, to);
            self.album_clip = Some(clip); // a copy can be pasted into more albums
        }
    }

    /// ＋: a new album under `parent` (0 = top), then let the user name it.
    /// Ctrl+N / empty-space menu: a new album in the album shown (the top in 무소속 and at the top).
    fn new_album_here(&mut self) {
        let parent = match self.cur_album() {
            Some(Some(p)) => p,
            _ => 0,
        };
        self.new_album_under(parent);
    }

    /// Del in the album view: pictures leave only the album shown; one that is
    /// in no other album goes to the imgr trash (keeping its album, so 되살리기
    /// brings it back here). Selected albums: ask, then delete them.
    fn delete_in_album_view(&mut self) {
        let ids = self.picked_pictures();
        if !ids.is_empty() {
            match self.cur_album() {
                Some(Some(x)) => self.take_out_here(&ids, x),
                // 무소속: these are in no album, so straight to the trash
                _ => self.set_trash(true),
            }
        }
        if !self.album_sel.is_empty() {
            let mut a: Vec<u32> = self.album_sel.iter().copied().collect();
            a.sort_unstable();
            self.modal = Some(Modal::AlbumDelete { ids: a });
        }
    }

    /// Pictures moved to the album view's top: they leave album `f` (and show at
    /// the top once they are in no album).
    pub(super) fn out_to_top(&mut self, ids: &[String], f: u32) {
        let name = self.album_label(Some(f));
        self.edit(ids, &trf!("'{name}'에서 맨 위로", "From '{name}' to top"), |im| im.albums.retain(|&a| a != f));
        self.notice(trf!("{}장을 '{name}'에서 뺌 (Ctrl+Z 되돌리기)", "Removed {} pictures from '{name}' (Ctrl+Z to undo)", ids.len()), false);
        self.sel.clear();
        self.refilter(true);
    }

    fn take_out_here(&mut self, ids: &[String], x: u32) {
        let only_here = |i: &Img| i.albums.iter().all(|&a| a == x);
        let last = ids.iter().filter(|id| self.img(id).is_some_and(|i| only_here(i))).count();
        let n = ids.len();
        let name = self.album_label(Some(x));
        self.edit(ids, &trf!("'{name}'에서 빼기", "Remove from '{name}'"), |im| {
            if im.albums.iter().all(|&a| a == x) {
                im.trash = true;
            } else {
                im.albums.retain(|&a| a != x);
            }
        });
        let msg = if last == 0 {
            trf!("{n}장을 '{name}'에서 뺌 (Ctrl+Z 되돌리기)", "Removed {n} pictures from '{name}' (Ctrl+Z to undo)")
        } else if last == n {
            trf!("{n}장 — 다른 앨범에 없어서 imgr 휴지통으로 (Ctrl+Z 되돌리기)", "{n} pictures — not in any other album, so moved to the imgr Trash (Ctrl+Z to undo)")
        } else {
            trf!("{}장은 '{name}'에서만 빼고, {last}장은 다른 앨범에 없어서 imgr 휴지통으로 (Ctrl+Z 되돌리기)", "{} removed from '{name}' only, {last} not in any other album so moved to the imgr Trash (Ctrl+Z to undo)", n - last)
        };
        self.notice(msg, false);
        let next = self.focus;
        self.sel.clear();
        self.refilter(true);
        self.focus = next.map(|f| f.min(self.shown.len().saturating_sub(1))).filter(|_| !self.shown.is_empty());
    }

    fn new_album_under(&mut self, parent: u32) {
        self.modal = Some(Modal::AlbumNew { parent, new: String::new(), put: Vec::new() });
    }

    fn delete_album(&mut self, id: u32) {
        let name = self.album_label(Some(id));
        let shown = self.cur_album() == Some(Some(id));
        let parent = self.db.album(id).map(|a| a.parent).unwrap_or(0);
        let n = self.db.delete_album(id);
        self.album_sel.remove(&id);
        // undo steps may hold the old album lists of pictures: they would bring back a dead id
        for st in self.undo.iter_mut() {
            for it in st.items.iter_mut() {
                it.albums.retain(|&a| a != id);
            }
        }
        if let Some(c) = self.album_clip.as_mut() {
            if c.from == Some(Some(id)) {
                c.from = None;
            }
        }
        self.mark_changed();
        self.notice(trf!("앨범 '{name}' 지움 (그림 {n}장은 그대로, 하위 앨범은 한 칸 위로)", "Deleted album '{name}' ({n} pictures kept, sub-albums moved up one level)"), false);
        if shown {
            self.set_query(if parent == 0 { String::new() } else { self.album_q(Some(parent)) });
        } else {
            self.refilter(true);
        }
    }

    /// One level up in the album view (the top: back to the plain list).
    fn album_up(&mut self) {
        self.album_sel.clear();
        match self.cur_album() {
            Some(Some(id)) => {
                let p = self.db.album(id).map(|a| a.parent).unwrap_or(0);
                self.open_album(if p == 0 { None } else { Some(Some(p)) });
            }
            Some(None) => self.open_album(None),
            None => self.view = View::Grid,
        }
    }

    /// Show an album (`Some(None)` = 앨범 없음, `None` = the album top) in the album view.
    fn open_album(&mut self, a: Option<Option<u32>>) {
        self.view = View::Albums;
        self.album_sel.clear();
        self.album_anchor = None;
        self.album_focus = None;
        self.sel.clear();
        self.set_query(match a {
            Some(x) => self.album_q(x),
            None => self.album_q(Some(0)),
        });
    }


    fn set_trash(&mut self, on: bool) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        self.edit(&ids, if on { tr("휴지통으로", "To Trash") } else { tr("되살리기", "Restore") }, |im| im.trash = on);
        self.notice(trf!("{}장 {}", "{} pictures {}", ids.len(), if on { tr("imgr 휴지통으로 (Ctrl+Z 되돌리기)", "moved to the imgr Trash (Ctrl+Z to undo)") } else { tr("되살림", "restored") }), false);
        let next = self.focus;
        if self.view == View::Compare {
            self.cmp_ids.retain(|id| !ids.contains(id));
            self.cmp_focus = self.cmp_focus.min(self.cmp_ids.len().saturating_sub(1));
        }
        self.refilter(true);
        // the trashed pictures left the list: keep the cursor at the same place
        let gone = !self.focus.is_some_and(|f| self.shown.get(f).is_some_and(|&i| !ids.contains(&self.db.images[i].id)));
        if gone || on {
            self.sel.clear();
            self.focus = next.map(|f| f.min(self.shown.len().saturating_sub(1))).filter(|_| !self.shown.is_empty());
            if let Some(f) = self.focus {
                self.sel.insert(self.db.images[self.shown[f]].id.clone());
                self.anchor = Some(f);
            }
        }
    }

    fn do_undo(&mut self) {
        let Some(step) = self.undo.pop() else {
            self.notice(tr("되돌릴 것이 없습니다", "Nothing to undo"), false);
            return;
        };
        for it in step.items {
            if let Some(&i) = self.id_index.get(&it.id) {
                let im = &mut self.db.images[i];
                im.score = it.score;
                im.tags = it.tags;
                im.trash = it.trash;
                im.preset = it.preset;
                im.preset_set = it.preset_set;
                im.albums = it.albums;
            }
        }
        self.mark_changed();
        self.mx_cache = None;
        self.refilter(true);
        self.notice(trf!("되돌림: {}", "Undone: {}", step.label), false);
    }

    fn empty_trash(&mut self) {
        let paths: Vec<(String, PathBuf)> = self.db.images.iter().filter(|i| i.trash && !i.missing).map(|i| (i.id.clone(), self.img_path(i))).collect();
        let mut ok = HashSet::new();
        let mut fail = 0;
        for (id, p) in paths {
            match trash::delete(&p) {
                Ok(()) => {
                    ok.insert(id);
                }
                Err(_) => fail += 1,
            }
        }
        let keep = self.focus_ids();
        self.db.images.retain(|i| !ok.contains(&i.id));
        self.mut_gen += 1;
        self.after_records_removed(keep);
        self.notice(trf!("윈도우 휴지통으로 {}장 보냄{}", "Sent {} pictures to the Windows Recycle Bin{}", ok.len(), if fail > 0 { trf!(", 실패 {fail}", ", {fail} failed") } else { String::new() }), fail > 0);
    }

    fn forget_missing(&mut self, ids: &[String]) {
        let ids: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        let before = self.db.images.len();
        let keep = self.focus_ids();
        // only records that are still missing now
        self.db.images.retain(|i| !(i.missing && ids.contains(i.id.as_str())));
        let n = before - self.db.images.len();
        self.mut_gen += 1;
        self.after_records_removed(keep);
        self.notice(trf!("사라진 그림 {n}장을 목록에서 지움", "Removed {n} missing pictures from the list"), false);
    }

    fn after_records_removed(&mut self, keep: (Option<String>, Option<String>)) {
        self.reindex();
        self.undo.clear();
        self.mark_changed();
        self.mx_cache = None;
        self.save_now();
        self.refilter_after_removal(keep);
    }

    /// Ids of the focused and anchor pictures; take them BEFORE records are removed.
    fn focus_ids(&self) -> (Option<String>, Option<String>) {
        let id = |p: Option<usize>| p.and_then(|p| self.shown.get(p)).and_then(|&i| self.db.images.get(i)).map(|im| im.id.clone());
        (id(self.focus), id(self.anchor))
    }

    /// Records were removed from db.images: `shown` holds stale indices, so
    /// rebuild it from scratch and put focus / anchor back by id.
    fn refilter_after_removal(&mut self, (focus, anchor): (Option<String>, Option<String>)) {
        self.shown.clear();
        self.focus = None;
        self.anchor = None;
        self.refilter(true);
        if let Some(p) = focus.and_then(|id| self.pos_of.get(&id).copied()) {
            self.focus = Some(p);
        }
        self.anchor = anchor.and_then(|id| self.pos_of.get(&id).copied()).or(self.focus);
    }

    /// Rename pictures on disk. names: (id, new file name).
    fn rename_images(&mut self, names: Vec<(String, String)>) -> Result<usize, String> {
        let mut done = 0;
        let mut errs = Vec::new();
        for (id, new) in names {
            let Some(&i) = self.id_index.get(&id) else { continue };
            let rel = self.db.images[i].path.clone();
            match db::rename_in_place(&rel, &new) {
                Ok(np) => {
                    let im = &mut self.db.images[i];
                    im.path = np;
                    let p = db::preset_of(im.name());
                    if !p.is_empty() && !im.preset_set {
                        im.preset = p;
                    }
                    if let Ok(md) = std::fs::metadata(db::src_dir().join(im.path.replace('/', "\\"))) {
                        im.mtime = db::mtime_of(&md);
                    }
                    done += 1;
                }
                Err(e) => errs.push(format!("{new}: {e}")),
            }
        }
        if done > 0 {
            self.mut_gen += 1;
            self.mark_changed();
            self.save_now();
            self.refilter(true);
        }
        if errs.is_empty() { Ok(done) } else { Err(trf!("{}장 바꿈, 실패 {}: {}", "{} renamed, {} failed: {}", done, errs.len(), errs[0])) }
    }

    // ------------------------------------------------------------ clipboard
    fn copy_files(&mut self) {
        let ids = self.selected_ids();
        let paths = self.paths_of(&ids);
        if paths.is_empty() {
            return;
        }
        match winshell::copy_files(&paths) {
            Ok(()) => self.notice(trf!("{}장 복사함 — 탐색기·업로드 창에 붙여넣기(Ctrl+V) 할 수 있음", "Copied {} pictures — paste into Explorer or an upload window with Ctrl+V", paths.len()), false),
            Err(e) => self.notice(trf!("복사 실패: {e}", "Copy failed: {e}"), true),
        }
    }

    fn copy_paths(&mut self) {
        let ids = self.selected_ids();
        let paths = self.paths_of(&ids);
        let text = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\r\n");
        if !text.is_empty() {
            let _ = winshell::copy_text(&text);
            self.notice(trf!("경로 {}개 복사함", "Copied {} paths", paths.len()), false);
        }
    }

    // ------------------------------------------------------------ selection
    fn click(&mut self, pos: usize, m: Modifiers) {
        let id = self.db.images[self.shown[pos]].id.clone();
        if self.view == View::Albums {
            let lead = self.album_lead();
            let nl = lead.len();
            if m.shift && nl > 0 {
                let a = self.cell_anchor(nl).unwrap_or(pos + nl);
                self.select_cells(a, pos + nl, &lead, m.ctrl);
                self.set_cell_cursor(pos + nl, nl, false);
                return;
            }
            self.album_focus = None;
            if !m.shift {
                self.album_anchor = None;
            }
        }
        if m.shift {
            let a = self.anchor.or(self.focus).unwrap_or(pos).min(self.shown.len().saturating_sub(1));
            if !m.ctrl {
                self.sel.clear();
            }
            for p in a.min(pos)..=a.max(pos) {
                self.sel.insert(self.db.images[self.shown[p]].id.clone());
            }
        } else if m.ctrl {
            if !self.sel.remove(&id) {
                self.sel.insert(id);
            }
            self.anchor = Some(pos);
        } else {
            self.sel.clear();
            self.sel.insert(id);
            self.anchor = Some(pos);
        }
        if !m.ctrl && !m.shift {
            self.album_sel.clear();
        }
        self.focus = Some(pos);
    }

    /// Select these pictures (those that are shown), focus the first.
    fn select_ids(&mut self, ids: &[String]) {
        self.sel.clear();
        for id in ids {
            if self.pos_of.contains_key(id) {
                self.sel.insert(id.clone());
            }
        }
        let first = ids.iter().find_map(|id| self.pos_of.get(id).copied());
        if first.is_some() {
            self.focus = first;
            self.anchor = first;
            self.scroll_to = first;
        }
    }

    fn move_focus(&mut self, delta: isize, shift: bool) {
        if self.view == View::Albums {
            return self.move_cell(delta, shift);
        }
        if self.shown.is_empty() {
            return;
        }
        let cur = self.focus.unwrap_or(0) as isize;
        let n = (cur + delta).clamp(0, self.shown.len() as isize - 1) as usize;
        if shift {
            let a = self.anchor.unwrap_or(cur as usize).min(self.shown.len() - 1);
            self.sel.clear();
            for p in a.min(n)..=a.max(n) {
                self.sel.insert(self.db.images[self.shown[p]].id.clone());
            }
            self.anchor = Some(a);
        } else {
            self.sel.clear();
            self.sel.insert(self.db.images[self.shown[n]].id.clone());
            self.anchor = Some(n);
            self.album_sel.clear();
        }
        self.focus = Some(n);
        self.scroll_to = Some(n);
        if !self.zp.actual {
            self.zp.reset();
        }
    }

    fn open_viewer(&mut self, from: View) {
        if self.focus.is_none() {
            return;
        }
        self.zp.reset();
        if from != View::Viewer {
            self.back_view = from;
        }
        self.view = View::Viewer;
    }

    /// Open a picture big from a view that is not the grid (matrix):
    /// the search is changed so ←/→ walk through the same group.
    fn open_in_viewer(&mut self, id: &str, query: Option<String>, from: View) {
        if let Some(q) = query {
            if q != self.query_text {
                self.query_text = q;
                self.refilter(false);
                self.settings.query = self.query_text.clone();
            }
        }
        if let Some(&p) = self.pos_of.get(id) {
            self.focus = Some(p);
            self.anchor = Some(p);
            self.sel.clear();
            self.sel.insert(id.to_string());
            self.open_viewer(from);
        }
    }

    fn open_compare_ids(&mut self, ids: Vec<String>) {
        if ids.len() < 2 {
            self.notice(tr("비교하려면 2장 이상 고르세요 (Ctrl/Shift+클릭 또는 빈 곳에서 끌기)", "Select 2 or more pictures to compare (Ctrl/Shift+click or drag on empty space)"), true);
            return;
        }
        if ids.len() > 12 {
            self.notice(trf!("{}장 중 앞의 12장만 비교합니다", "Comparing only the first 12 of {} pictures", ids.len()), false);
        }
        if self.view != View::Compare {
            self.back_view = self.view;
        }
        self.cmp_ids = ids.into_iter().take(12).collect();
        self.cmp_focus = 0;
        self.zp.reset();
        self.view = View::Compare;
    }

    fn open_compare(&mut self) {
        if self.view == View::Compare {
            return;
        }
        let ids = self.selected_ids();
        self.open_compare_ids(ids);
    }

    fn go_back(&mut self) {
        let to = match self.view {
            View::Viewer | View::Compare => self.back_view,
            _ => View::Grid,
        };
        self.view = if to == View::Viewer || to == View::Compare { View::Grid } else { to };
        if matches!(self.view, View::Grid | View::Albums) {
            if let Some(f) = self.focus {
                self.scroll_to = Some(f);
            }
        }
    }

    fn set_fullscreen(&mut self, on: bool) {
        self.fullscreen = on;
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }

    // ------------------------------------------------------------ keys
    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.modal.is_some() {
            return;
        }
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::F)) {
            self.focus_search = true;
            return;
        }
        if ctx.text_edit_focused() || ctx.any_popup_open() {
            return;
        }
        let evs: Vec<(Key, Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        // eframe turns Ctrl+C / X / V into Copy / Cut / Paste events instead of keys
        // (an injected key from ctl arrives as a key: both lead to the same place)
        let (mut copy, mut cut, mut paste) = ctx.input(|i| {
            let c = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
            let x = i.events.iter().any(|e| matches!(e, egui::Event::Cut));
            let p = i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)));
            (c, x, p)
        });
        let shift = ctx.input(|i| i.modifiers.shift);
        for (k, m) in &evs {
            if m.ctrl {
                match k {
                    Key::C => copy = true,
                    Key::X => cut = true,
                    Key::V => paste = true,
                    _ => {}
                }
            }
        }
        if copy {
            if shift { self.copy_paths() } else { self.album_copy(false) }
        }
        if cut {
            self.album_copy(true);
        }
        if paste {
            self.paste_any();
        }
        for (k, m) in evs {
            self.on_key(k, m);
        }
        // mouse back / forward buttons
        let (back, fwd) = ctx.input(|i| (i.pointer.button_pressed(egui::PointerButton::Extra1), i.pointer.button_pressed(egui::PointerButton::Extra2)));
        if back {
            if self.view == View::Viewer {
                self.move_focus(-1, false);
            } else if self.view == View::Albums {
                self.album_up();
            } else if self.view != View::Grid {
                self.go_back();
            }
        }
        if fwd && self.view == View::Viewer {
            self.move_focus(1, false);
        }
        // Ctrl+wheel: thumbnail size in the grid / matrix / albums
        if matches!(self.view, View::Grid | View::Matrix | View::Albums) {
            // egui turns Ctrl+wheel into a zoom gesture (and does not scroll)
            let z = ctx.input(|i| i.zoom_delta());
            if (z - 1.0).abs() > 0.001 && self.modal.is_none() {
                self.settings.thumb_size = (self.settings.thumb_size * z.clamp(0.8, 1.25)).clamp(70.0, 520.0).round();
            }
        }
    }

    fn on_key(&mut self, k: Key, m: Modifiers) {
        let grid_like = matches!(self.view, View::Grid | View::Albums);
        let cols = if grid_like { self.cols.max(1) as isize } else { 1 };
        match k {
            Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4 | Key::Num5 | Key::Num6 if !m.ctrl => {
                let n = match k {
                    Key::Num1 => 1,
                    Key::Num2 => 2,
                    Key::Num3 => 3,
                    Key::Num4 => 4,
                    Key::Num5 => 5,
                    _ => 6,
                };
                self.set_score(n);
            }
            // big viewer: ` shows / hides the text lines at the top and bottom
            Key::Backtick if !m.ctrl && self.view == View::Viewer => self.settings.viewer_text = !self.settings.viewer_text,
            Key::Num0 | Key::Backtick if !m.ctrl => self.set_score(0),
            Key::ArrowRight | Key::ArrowLeft | Key::ArrowDown | Key::ArrowUp if self.view == View::Matrix => {
                let (dr, dc) = match k {
                    Key::ArrowRight => (0, 1),
                    Key::ArrowLeft => (0, -1),
                    Key::ArrowDown => (1, 0),
                    _ => (-1, 0),
                };
                self.matrix_move(dr, dc);
            }
            Key::ArrowRight => {
                if self.view == View::Compare {
                    self.cmp_focus = (self.cmp_focus + 1).min(self.cmp_ids.len().saturating_sub(1));
                } else {
                    self.move_focus(1, m.shift);
                }
            }
            Key::ArrowLeft => {
                if self.view == View::Compare {
                    self.cmp_focus = self.cmp_focus.saturating_sub(1);
                } else {
                    self.move_focus(-1, m.shift);
                }
            }
            // album view: Ctrl+↑ / Alt+↑ / Backspace = up one album
            Key::ArrowUp if self.view == View::Albums && (m.ctrl || m.alt) => self.album_up(),
            Key::Backspace if self.view == View::Albums => self.album_up(),
            Key::ArrowDown if grid_like => self.move_focus(cols, m.shift),
            Key::ArrowUp if grid_like => self.move_focus(-cols, m.shift),
            Key::PageDown if matches!(self.view, View::Grid | View::Viewer | View::Albums) => {
                let rows = if grid_like { (self.view_h / self.row_h).floor().max(1.0) as isize } else { 10 };
                self.move_focus(cols * rows, m.shift)
            }
            Key::PageUp if matches!(self.view, View::Grid | View::Viewer | View::Albums) => {
                let rows = if grid_like { (self.view_h / self.row_h).floor().max(1.0) as isize } else { 10 };
                self.move_focus(-cols * rows, m.shift)
            }
            Key::Home if matches!(self.view, View::Grid | View::Viewer | View::Albums) => self.move_focus(-(1 << 40), m.shift),
            Key::End if matches!(self.view, View::Grid | View::Viewer | View::Albums) => self.move_focus(1 << 40, m.shift),
            Key::A if m.ctrl => {
                self.sel = self.shown.iter().map(|&i| self.db.images[i].id.clone()).collect();
                if self.view == View::Albums {
                    self.album_sel = self.album_lead().into_iter().flatten().collect();
                }
            }
            Key::Z if m.ctrl => self.do_undo(),
            Key::Z if !m.ctrl && self.view == View::Viewer => {
                self.zp.actual = !self.zp.actual;
                self.zp.zoom = 1.0;
                self.zp.center = vec2(0.5, 0.5);
            }
            Key::E if m.ctrl => self.open_export(),
            Key::C | Key::X | Key::V if m.ctrl => {} // handled above (Copy / Cut / Paste)
            Key::Escape if self.pic_drag.is_some() => {
                self.pic_drag = None;
                self.album_drag.clear();
            }
            Key::Enter if self.view == View::Albums && self.sel.is_empty() && self.album_sel.len() == 1 => {
                let id = *self.album_sel.iter().next().unwrap();
                self.open_album(Some(Some(id)));
            }
            // the cursor on 무소속 (nothing selected): open it
            Key::Enter if self.view == View::Albums && self.sel.is_empty() && self.album_sel.is_empty() && self.album_focus.is_some() => {
                let lead = self.album_lead();
                if let Some(&t) = self.album_focus.and_then(|g| lead.get(g)) {
                    self.open_album(Some(t));
                }
            }
            Key::Enter => match self.view {
                View::Grid | View::Albums => self.open_viewer(self.view),
                View::Matrix => {
                    if let Some(id) = self.matrix_focus_id() {
                        self.open_in_viewer(&id, None, View::Matrix);
                    }
                }
                View::Compare => {
                    if let Some(id) = self.cmp_ids.get(self.cmp_focus).cloned() {
                        self.open_in_viewer(&id, None, View::Compare);
                    }
                }
                View::Viewer => self.go_back(),
            },
            Key::Space if matches!(self.view, View::Grid | View::Viewer | View::Albums) => {
                if self.view == View::Viewer { self.go_back() } else { self.open_viewer(self.view) }
            }
            Key::Escape => {
                if self.fullscreen {
                    self.set_fullscreen(false);
                } else if self.view == View::Albums {
                    if self.sel.is_empty() && self.album_sel.is_empty() {
                        self.album_up()
                    } else {
                        self.sel.clear();
                        self.album_sel.clear();
                    }
                } else if self.view != View::Grid {
                    self.go_back();
                } else {
                    self.sel.clear();
                }
            }
            Key::C if !m.ctrl => match self.view {
                View::Compare => self.go_back(),
                View::Matrix => {
                    let ids = self.matrix_row_ids();
                    self.open_compare_ids(ids);
                }
                _ => self.open_compare(),
            },
            Key::M if !m.ctrl => {
                self.view = if self.view == View::Matrix { View::Grid } else { View::Matrix };
            }
            Key::A if !m.ctrl && !m.shift => {
                self.view = if self.view == View::Albums { View::Grid } else { View::Albums };
            }
            Key::T if !m.ctrl => {
                if self.fullscreen {
                    self.set_fullscreen(false);
                }
                self.settings.show_right = true;
                self.focus_tag_input = true;
            }
            Key::F2 if self.view == View::Albums && self.sel.is_empty() && self.album_sel.len() == 1 => {
                let id = *self.album_sel.iter().next().unwrap();
                let name = self.album_label(Some(id));
                self.modal = Some(Modal::AlbumRename { id, new: name });
            }
            Key::F2 => self.open_rename(),
            Key::Slash => self.focus_search = true,
            Key::Delete if self.view == View::Albums => self.delete_in_album_view(),
            Key::N if m.ctrl && self.view == View::Albums => self.new_album_here(),
            Key::Delete => {
                let in_trash = query::has_word(&self.query_text, "is:trash");
                self.set_trash(!in_trash);
            }
            Key::F1 => self.modal = Some(Modal::Help),
            Key::F5 => self.start_scan(false),
            Key::F11 => self.set_fullscreen(!self.fullscreen),
            Key::OpenBracket => self.settings.show_left = !self.settings.show_left,
            Key::CloseBracket => self.settings.show_right = !self.settings.show_right,
            Key::R if !m.ctrl => self.zp.reset(),
            _ => {}
        }
    }

    fn open_rename(&mut self) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        let name = self.img(&ids[0]).map(|i| i.name().to_string()).unwrap_or_default();
        self.modal = Some(Modal::Rename { ids, name, fresh: true, upload: false, pattern: "name_###".into(), start: 1, error: String::new() });
    }

    fn open_export(&mut self) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        let s = &self.settings;
        self.modal = Some(Modal::Export {
            ids,
            dir: s.export_dir.clone(),
            fmt: s.export_fmt.clone(),
            quality: s.export_quality,
            upload: s.export_upload_names,
            tag: s.export_tag.clone(),
        });
    }

    fn mark(&mut self, name: &str, r: Rect) {
        self.layout.insert(name.to_string(), r);
    }
}

const HELP: &str = "조건은 콤마로 나눔 = 모두 맞는 것 (AND).  앞에 - = 빼기.  a|b = 둘 중 하나 (OR).
long hair, scar   프롬프트 태그를 통째로 (띄어쓰기 = _, 대소문자·가중치 (tag:1.2) 무시)
__scar__  __scar  scar__   scar가 들어간 / scar로 끝나는 / scar로 시작하는 태그

#pixiv            해시태그가 있는 것        -#pixiv   없는 것
#올림|#올릴것       둘 중 하나라도
score:5  score>=4  score<3  score:0(점수 없음)
preset:photoB        파일 이름 g_photoB_… 의 프리셋
ckpt:obsess       체크포인트 이름에 들어간 글자      lora:pastel
seed:12345        같은 시드 (체크포인트·프리셋 비교)
folder:00001      name:003640
album:여행  album:none   여행 앨범에 바로 든 그림 / 무소속(어느 앨범에도 없는 그림)
album:여행+         여행과 그 하위 앨범들의 그림 (왼쪽 앨범을 누르면 넣어 줌)
album:여행/바다      이름이 같은 앨범이 또 있으면 경로로   \"album:내 앨범\"  띄어쓰기는 따옴표
album:top          앨범 뷰의 맨 위 (맨 위 앨범들과 무소속 그림)
is:trash  is:unscored  is:scored  is:tagged  is:untagged  is:missing
added:today  added:yesterday  added:3d(최근 3일)  added:11-15  added>=2026-11-01   (imgr에 넣은 날)
date:today  date<11-10 …                                                      (파일 시각 = 만든 날)
sort:new  old  name  score  seed  size  random
그 밖의 글자        프롬프트 태그 (해시태그·프리셋 이름도). 파일 이름은 name:, 체크포인트는 ckpt:
#a, album:x 처럼 기호 붙은 낱말은 띄어쓰기로도 나뉨

왼쪽 목록을 누르면 검색에 넣고 빼 줍니다: 검색칸 왼쪽 AND = 새 조건, OR = 마지막 조건에 | 로.
Ctrl+클릭 = 그 종류는 이것만.  오른쪽 클릭 = 메뉴.

키
  1~6 점수  0 지우기 (크게 보기에서는 점수 주면 다음 그림으로)
  T 태그 입력   F2 이름 바꾸기   Del 휴지통   Ctrl+Z 되돌리기
  Ctrl+C 복사 (탐색기·업로드 창에 붙여넣기도 됨)   Ctrl+X 잘라 두기   Ctrl+Shift+C 경로 복사
  Ctrl+V  열린 앨범에: Ctrl+C 한 그림은 같이 들어감(여러 앨범), Ctrl+X 한 그림은 옮겨짐
          탐색기에서 복사한 파일은 가져오기 (앨범이 열려 있으면 거기에도 넣음)
  Ctrl+E 내보내기 (업로드용: jpg·메타데이터 지움·무작위 이름·태그 붙이기)
  Enter/Space 크게   C 비교   M 표 비교 (시드 × 프리셋)   A 앨범   Esc 뒤로 (앨범: 한 칸 위)
  앨범 뷰: 클릭·Enter 앨범 열기, Ctrl·Shift+클릭·빈 곳에서 끌기 = 고르기, F2 이름, Del 고른 앨범 지우기,
          Ctrl+N·빈 곳 오른쪽 클릭 새 앨범, 그림 Del = 이 앨범에서만 빼기 (마지막 앨범이면 imgr 휴지통),
            다른 앨범에 끌어 놓기·Ctrl+X → Ctrl+V 옮기기, Ctrl+↑·Alt+↑·Backspace 한 칸 위,
            방향키·PageUp/Down·Home/End = 앨범 칸과 그림을 한 줄로 이동
  ←→↑↓ 이동 (Shift 범위)   Ctrl+A 모두   빈 곳에서 끌기 = 여러 장 고르기
  고른 그림을 끌어서 앨범(왼쪽 목록·앨범 화면)에 놓기 = 그 앨범에 넣기 (Shift 누른 채 놓기 = 옮기기; 앨범 뷰에서는 놓으면 옮기기, Ctrl = 복사),  창 밖으로 끌면 다른 프로그램(탐색기·업로드)에 놓기
  [ ] 왼쪽/오른쪽 칸 숨기기   F11 전체 화면   F5 다시 읽기   Ctrl+휠 썸네일 크기
  크게 보기: 휠 확대 · 끌어서 이동 · R 맞춤 · Z 실제 크기 · 마우스 앞/뒤 버튼 넘기기 · ` 위아래 글 숨기기/보이기
  왼쪽 태그 여부: 워크플로(ComfyUI 정보 있는 그림) / 태그없음 — 둘 다 켜면 모두 (앨범 뷰·휴지통에는 적용 안 됨)";

const HELP_EN: &str = "Conditions are separated by commas = all must match (AND).  A leading - = exclude.  a|b = either one (OR).
long hair, scar   whole prompt tags (space = _, case and weights (tag:1.2) ignored)
__scar__  __scar  scar__   tags containing / ending with / starting with scar

#pixiv            pictures with the hashtag        -#pixiv   without it
#posted|#todo      either hashtag
score:5  score>=4  score<3  score:0 (no score)
preset:photoB        preset of files named g_photoB_...
ckpt:obsess       text contained in the checkpoint name      lora:pastel
seed:12345        same seed (compare checkpoints / presets)
folder:00001      name:003640
album:trip  album:none   pictures directly in album trip / in no album at all
album:trip+         pictures of trip and its sub-albums (clicking an album on the left puts this in)
album:trip/sea      with the path, if another album has the same name   \"album:my trip\"  quote names with spaces
album:top          top of the Album view (top-level albums and pictures in no album)
is:trash  is:unscored  is:scored  is:tagged  is:untagged  is:missing
added:today  added:yesterday  added:3d (last 3 days)  added:11-15  added>=2026-11-01   (day added to imgr)
date:today  date<11-10 ...                                                      (file time = day created)
sort:new  old  name  score  seed  size  random
any other text        a prompt tag (or a hashtag / preset by name). File names: name:, checkpoints: ckpt:
words with a sign like #a or album:x are also split by spaces

Clicking an item in the left list adds / removes it: AND (left of the search box) = a new condition, OR = joined to the last one with |.
Ctrl+click = only this of its kind.  Right click = menu.

Keys
  1~6 score  0 clear (in the Viewer, scoring moves on to the next picture)
  T enter tag   F2 rename   Del Trash   Ctrl+Z undo
  Ctrl+C copy (can be pasted into Explorer / upload windows)   Ctrl+X cut   Ctrl+Shift+C copy path
  Ctrl+V  into an open album: pictures copied with Ctrl+C are added too (several albums), pictures cut with Ctrl+X are moved
          files copied in Explorer are imported (and added to the open album, if any)
  Ctrl+E Export (for uploading: jpg, metadata removed, random names, add a tag)
  Enter/Space Viewer   C compare   M table compare (seed x preset)   A Albums   Esc back (Albums: up one level)
  Album view: click / Enter opens an album, Ctrl / Shift+click / drag on empty space = select, F2 rename, Del delete selected albums,
          Ctrl+N / right click on empty space = new album, Del on a picture = remove from this album only (Trash if it was the last album),
            drag onto another album / Ctrl+X then Ctrl+V = move, Ctrl+Up / Alt+Up / Backspace = up one level,
            arrow keys / PageUp/Down / Home/End = move through album cells and pictures as one line
  Arrows move (Shift = range)   Ctrl+A all   drag on empty space = select several
  Drag selected pictures onto an album (left list / Album view) = add them to it (hold Shift while dropping = move; in the Album view dropping moves, Ctrl = copy),  drag out of the window to drop into another program (Explorer / upload)
  [ ] hide left / right pane   F11 full screen   F5 reload   Ctrl+wheel thumbnail size
  Viewer: wheel zoom / drag to pan / R fit / Z actual size / mouse back/forward buttons = previous/next / ` hide or show the top and bottom text
  Left tag filter: Workflow (pictures with ComfyUI info) / No tags - turn both on to show everything (not applied in the Album view or Trash)";

/// The help text (F1) in the current language.
pub(crate) fn help_text() -> &'static str {
    if crate::i18n::en() { HELP_EN } else { HELP }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.offscreen {
            // test window: never focused for real, but behave as if it were
            raw.focused = true;
        }
        // clipboard keys arrive as plain keys (main.rs): a text box still needs them as Copy / Cut / Paste
        if ctx.text_edit_focused() {
            for e in raw.events.iter_mut() {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                    let (k, m) = (*key, *modifiers);
                    let new = if m.command && !m.alt && k == Key::C || m.ctrl && k == Key::Insert {
                        Some(egui::Event::Copy)
                    } else if m.command && !m.alt && k == Key::X || m.shift && k == Key::Delete {
                        Some(egui::Event::Cut)
                    } else if m.command && !m.alt && k == Key::V || m.shift && k == Key::Insert {
                        winshell::clipboard_text().map(|t| egui::Event::Paste(t.replace("\r\n", "\n")))
                    } else {
                        None
                    };
                    if let Some(n) = new {
                        *e = n;
                    }
                }
            }
        }
        for p in self.inject_drop.drain(..) {
            raw.dropped_files.push(Arc::new(CtlDrop(p)));
        }
        if let Some(step) = self.inject.pop_front() {
            if let Some(m) = step.mods {
                raw.events.push(egui::Event::ModifiersChanged(m));
            }
            raw.events.extend(step.events);
            if let Some(r) = step.reply {
                self.inject_replies.push(r);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame_no += 1;
        self.layout.clear();
        ctx.input(|i| {
            let vp = i.viewport();
            if let Some(r) = vp.inner_rect {
                self.window_size = r.size();
            }
            if let Some(r) = vp.outer_rect {
                self.window_pos = Some(r.min);
            }
            self.maximized = vp.maximized.unwrap_or(false);
        });
        if self.first_frame {
            self.first_frame = false;
            if self.settings.win_max && !self.offscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
        }
        if self.counts_dirty {
            self.recount();
        }
        self.album_place_fix();
        self.handle_keys(&ctx);
        // files dropped on the window -> import (copy)
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            // dropped while an album is shown: the pictures go into it too
            let to = self.cur_album().filter(|a| a.is_some());
            self.start_import_to(dropped, false, to);
        }
        self.album_drop.clear();
        let hovering_files = ctx.input(|i| !i.raw.hovered_files.is_empty());

        if !self.fullscreen {
            egui::Panel::top("top").show(ui, |ui| {
                ui.add_space(4.0);
                self.ui_top(ui);
                ui.add_space(3.0);
            });
            egui::Panel::bottom("status").show(ui, |ui| self.ui_status(ui));
            if self.settings.show_left {
                let r = egui::Panel::left("left").default_size(self.settings.left_w).size_range(150.0..=560.0).show(ui, |ui| {
                    // egui sizes a panel to its content: keep the width the user dragged to
                    ui.set_min_width(ui.available_width());
                    self.ui_left(ui)
                });
                self.settings.left_w = r.response.rect.width().round();
                self.mark("left_panel", r.response.rect);
            }
            if self.settings.show_right {
                let mut panel = egui::Panel::right("right").default_size(self.settings.right_w).size_range(220.0..=900.0);
                if self.corner_drag {
                    panel = panel.exact_size(self.settings.right_w);
                }
                let r = panel.show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    self.ui_right(ui)
                });
                if !self.corner_drag {
                    self.settings.right_w = r.response.rect.width().round();
                }
                self.mark("right_panel", r.response.rect);
            }
        }
        egui::CentralPanel::default_margins().frame(egui::Frame::new().fill(BG).inner_margin(4.0)).show(ui, |ui| {
            match self.view {
                View::Grid => self.ui_grid(ui),
                View::Albums => self.ui_albums(ui),
                View::Viewer => self.ui_viewer(ui),
                View::Compare => self.ui_compare(ui),
                View::Matrix => self.ui_matrix(ui),
            }
            if hovering_files {
                let r = ui.max_rect();
                ui.painter().rect_filled(r, 6.0, Color32::from_rgba_unmultiplied(40, 70, 110, 90));
                let msg = match self.cur_album() {
                    Some(Some(a)) => trf!("놓으면 imgs로 복사하고 앨범 '{}'에 넣습니다", "Drop to copy into imgs and add to album '{}'", self.album_label(Some(a))),
                    _ => tr("놓으면 imgs로 복사해서 넣습니다", "Drop to copy into imgs").to_string(),
                };
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, msg, egui::FontId::proportional(22.0), Color32::WHITE);
            }
        });
        self.pic_drag_frame(&ctx);
        self.ui_modal(&ctx);
        self.handle_ctl(&ctx);
        if !self.inject_replies.is_empty() && self.inject.is_empty() {
            let st = self.state_json();
            for r in self.inject_replies.drain(..) {
                let _ = r.send(st.clone());
            }
        }
        if !self.inject.is_empty() {
            ctx.request_repaint();
        }
        if self.scan_progress.is_some() || self.import_progress.is_some() || self.export_progress.is_some() || self.marquee.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            // periodic jobs need a frame now and then even when nothing happens
            ctx.request_repaint_after(Duration::from_secs(5));
        }
    }

    /// Runs before every ui() and also while the window is minimized / hidden,
    /// so saving, scans, watch folders and ctl keep going.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background();
        let asked = self.focus_asked.load(Ordering::Relaxed);
        if asked != self.focus_seen {
            self.focus_seen = asked;
            if !self.offscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        self.sync_album_names();
        let visible = ctx.input(|i| i.viewport().visible()).unwrap_or(true);
        if !visible {
            self.handle_ctl(ctx);
            ctx.request_repaint_after(Duration::from_secs(5));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if !self.offscreen {
            self.settings.win_max = self.maximized;
            if !self.maximized && !self.fullscreen {
                self.settings.win_w = self.window_size.x;
                self.settings.win_h = self.window_size.y;
                if let Some(p) = self.window_pos {
                    self.settings.win_x = Some(p.x);
                    self.settings.win_y = Some(p.y);
                }
            }
        }
        self.settings.query = self.query_text.clone();
        self.settings.last_focus = self.focus_img().map(|i| i.id.clone()).unwrap_or_default();
        self.save_blocking();
    }
}
