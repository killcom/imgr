use serde::{Deserialize, Serialize};

use crate::db::data_dir;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    /// Thumbnail picture height in the grid.
    pub thumb_size: f32,
    pub query: String,
    pub sort: String,
    pub show_names: bool,
    /// Hashtags used lately (newest first), shown as one-click chips.
    pub recent_tags: Vec<String>,
    /// Searches used lately (newest first).
    pub search_history: Vec<String>,
    pub import_dir: String,
    pub import_move: bool,
    pub win_w: f32,
    pub win_h: f32,
    pub win_x: Option<f32>,
    pub win_y: Option<f32>,
    pub win_max: bool,
    pub left_w: f32,
    pub right_w: f32,
    /// Right panel preview: at most this part of the window height.
    pub preview_frac: f32,
    pub show_left: bool,
    pub show_right: bool,
    /// In the big viewer, giving a score moves on to the next picture.
    pub auto_advance: bool,
    /// Picture that had the focus when imgr was closed.
    pub last_focus: String,
    pub export_dir: String,
    /// "keep" | "jpg" | "png"
    pub export_fmt: String,
    pub export_quality: u8,
    pub export_upload_names: bool,
    pub export_tag: String,
    /// Folders checked every minute; new pictures there are imported.
    pub watch_dirs: Vec<String>,
    pub watch_move: bool,
    /// Matrix view columns: "preset" | "ckpt" | "lora"
    pub matrix_col: String,
    /// Big viewer: the text lines at the top and bottom (` toggles).
    pub viewer_text: bool,
    /// Pictures per imgs sub folder (00001, 00002 ...) before new ones go to the next.
    pub per_folder: usize,
    /// 검색 뷰 › 태그 여부: show pictures with a workflow (ComfyUI info) / without.
    pub kind_wf: bool,
    pub kind_plain: bool,
    /// Take commands from scripts on this PC (127.0.0.1:47811, tools/imgr_ctl.py).
    /// Off by default; env IMGR_CTL=1 turns it on for one run. Read at start.
    pub ctl: bool,
    /// [AND | OR] left of the search box: how a click in 검색 뷰 adds its item.
    pub search_or: bool,
    /// Del and Shift+Del swapped (Del = to the imgr Trash, Shift+Del = out of this album only).
    pub del_swap: bool,
    /// Screen language: "ko" or "en" (first start: the language of Windows).
    pub lang: String,
    /// 비교열: rows of pictures compared side by side, one row per "add" (kept across restarts)
    pub cmp_rows: Vec<CmpRow>,
    /// 비교열 screen: how many rows fit on one screen (1-3)
    pub cmp_per_screen: u8,
}

/// One 비교열 row: the pictures added together (a selection or an album).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct CmpRow {
    /// where it came from (album path, search, ...)
    pub label: String,
    pub ids: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            thumb_size: 180.0,
            query: String::new(),
            sort: "new".into(),
            show_names: true,
            recent_tags: Vec::new(),
            search_history: Vec::new(),
            import_dir: String::new(),
            import_move: false,
            win_w: 1500.0,
            win_h: 900.0,
            win_x: None,
            win_y: None,
            win_max: false,
            left_w: 230.0,
            right_w: 340.0,
            preview_frac: 0.45,
            show_left: true,
            show_right: true,
            auto_advance: true,
            last_focus: String::new(),
            export_dir: String::new(),
            export_fmt: "jpg".into(),
            export_quality: 92,
            export_upload_names: false,
            export_tag: String::new(),
            watch_dirs: Vec::new(),
            watch_move: false,
            matrix_col: "preset".into(),
            viewer_text: true,
            per_folder: 2000,
            kind_wf: true,
            kind_plain: false,
            search_or: false,
            del_swap: false,
            ctl: false,
            lang: crate::i18n::system_lang(),
            cmp_rows: Vec::new(),
            cmp_per_screen: 2,
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read(data_dir().join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        let d = data_dir();
        let _ = std::fs::create_dir_all(&d);
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let tmp = d.join("settings.json.tmp");
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(tmp, d.join("settings.json"));
            }
        }
    }
    pub fn push_recent(&mut self, tag: &str) {
        self.recent_tags.retain(|t| t != tag);
        self.recent_tags.insert(0, tag.to_string());
        self.recent_tags.truncate(16);
    }
    pub fn push_search(&mut self, q: &str) {
        let q = q.trim();
        if q.is_empty() {
            return;
        }
        self.search_history.retain(|t| t != q);
        self.search_history.insert(0, q.to_string());
        self.search_history.truncate(15);
    }
}
