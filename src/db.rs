//! Image library: a record per picture, kept in data/imgr.db (store.rs; db.json up to v0.16).
//!
//! A picture is identified by a hash of its bytes, so tags and scores survive
//! renames and moves inside imgs. Files that disappear are kept as
//! `missing` (hidden by default) so their tags come back if the file returns.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif"];
/// Pictures per imgs sub folder (00001, 00002, ...): set from the settings.
static PER_FOLDER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(2000);
pub fn set_per_folder(n: usize) {
    PER_FOLDER.store(n.max(1), std::sync::atomic::Ordering::Relaxed);
}
pub fn per_folder() -> usize {
    PER_FOLDER.load(std::sync::atomic::Ordering::Relaxed)
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Hash)]
#[serde(default)]
pub struct Img {
    pub id: String,
    /// Path relative to imgs, with '/' separators.
    pub path: String,
    pub size: u64,
    pub mtime: u64,
    pub w: u32,
    pub h: u32,
    /// When the picture entered the library (unix seconds).
    pub added: i64,
    /// 0 = no score, 1..=6.
    pub score: u8,
    /// Hashtags without '#', lower case.
    pub tags: Vec<String>,
    /// In imgr's own trash (file untouched until the trash is emptied).
    pub trash: bool,
    pub missing: bool,
    /// From the file name g_<preset>_<number>, or set by hand ("" = 없음).
    pub preset: String,
    /// Set by hand: file names no longer change it.
    pub preset_set: bool,
    /// Read from the ComfyUI prompt stored in the PNG.
    pub ckpt: String,
    pub loras: Vec<String>,
    pub prompt: String,
    pub seed: Option<u64>,
    pub meta_read: bool,
    /// Albums (imgr's own sorting, not real folders) this picture is in: album ids.
    pub albums: Vec<u32>,
}

impl Img {
    /// Made by ComfyUI with its info kept (prompt / checkpoint / seed); not read yet counts too.
    pub fn has_workflow(&self) -> bool {
        !self.meta_read || !self.prompt.is_empty() || !self.ckpt.is_empty() || self.seed.is_some()
    }
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
    pub fn folder(&self) -> &str {
        match self.path.rfind('/') {
            Some(i) => &self.path[..i],
            None => "",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Hash)]
pub struct SavedFilter {
    pub name: String,
    pub query: String,
}

/// An album: a user-made group of pictures inside imgr (not a folder on disk).
/// A picture can be in several albums; albums can hold sub albums.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Hash)]
#[serde(default)]
pub struct Album {
    /// 1, 2, 3 ... never reused
    pub id: u32,
    pub name: String,
    /// 0 = top level
    pub parent: u32,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Db {
    pub version: u32,
    pub images: Vec<Img>,
    pub albums: Vec<Album>,
    /// the old presets were turned into albums once (2026-11-23)
    pub presets_to_albums: bool,
    /// "필터태그": named searches shown in the side panel.
    pub saved: Vec<SavedFilter>,
    /// How PNG info was read (META_VER). Older: pictures without checkpoint or prompt are read again once.
    pub meta_ver: u32,
    /// fields written by a newer imgr: kept as they are when an older exe saves
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
    /// the library could not be read: never save over it
    #[serde(skip)]
    pub read_only: bool,
    /// fingerprint of each picture as last saved (only changed ones are written)
    #[serde(skip)]
    saved_fp: HashMap<String, u64>,
    /// albums / filters / small values as last saved; None = not saved yet
    #[serde(skip)]
    misc_fp: Option<u64>,
    /// next save rewrites everything (first write, or a save failed)
    #[serde(skip)]
    need_full: bool,
    /// this start moved db.json into imgr.db: how many pictures (a plain notice, not a warning)
    #[serde(skip)]
    pub moved_from_json: Option<usize>,
}

fn fp_of<T: std::hash::Hash>(v: &T) -> u64 {
    use std::hash::{BuildHasher, BuildHasherDefault};
    BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default().hash_one(v)
}

/// Version of the id hash (2 = the fixed FNV/CRC hash below).
pub const DB_VERSION: u32 = 2;
/// Version of the PNG info reader (2 = follows wires: UNETLoader models, prompts from string nodes; v0.15).
pub const META_VER: u32 = 2;

pub fn root_dir() -> PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        if let Some(r) = std::env::var_os("IMGR_ROOT") {
            return PathBuf::from(r);
        }
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."))
    })
    .clone()
}

/// Case-insensitive "is `p` inside `dir`" (Windows paths).
pub fn path_inside(p: &Path, dir: &Path) -> bool {
    let a = p.to_string_lossy().replace('/', "\\").to_lowercase();
    let b = dir.to_string_lossy().replace('/', "\\").to_lowercase();
    let b = b.trim_end_matches('\\');
    a == b || a.starts_with(&format!("{b}\\"))
}
/// Folder the pictures live in: `imgs` (called `imgsrc` up to v0.03). An old
/// `imgsrc` with no `imgs` beside it is renamed on first use; if that fails
/// (folder open elsewhere) the old one is used for this run.
pub fn src_dir() -> PathBuf {
    static SRC: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    SRC.get_or_init(|| pick_src(&root_dir())).clone()
}

pub fn pick_src(root: &Path) -> PathBuf {
    let new = root.join("imgs");
    let old = root.join("imgsrc");
    if !new.exists() && old.is_dir() && std::fs::rename(&old, &new).is_err() {
        return old;
    }
    new
}
pub fn data_dir() -> PathBuf {
    root_dir().join("data")
}

impl Db {
    /// Load the library (data/imgr.db). Returns the db and a warning for the user.
    /// No imgr.db but a db.json (imgr up to v0.16): it is moved into imgr.db once
    /// and checked field by field. A file that exists but cannot be read is never
    /// replaced: the db is opened read-only (nothing is saved) so the data stays on disk.
    pub fn load() -> (Db, Option<String>) {
        let d = data_dir();
        let sq = d.join(crate::store::FILE);
        let js = d.join("db.json");
        let ro = |why: String| (Db { read_only: true, version: DB_VERSION, meta_ver: META_VER, ..Default::default() }, Some(why));
        let (mut db, note) = if sq.exists() {
            match crate::store::open(&sq).map_err(|e| e.to_string()).and_then(|c| crate::store::load(&c)) {
                Ok(mut db) => {
                    let note = db.read_only.then(|| crate::i18n::tr("imgr.db가 이 판보다 새 imgr로 저장돼 있어 읽기만 합니다", "imgr.db was saved by a newer imgr: opened read-only").to_string());
                    db.mark_all_saved();
                    (db, note)
                }
                Err(e) => return ro(crate::trf!("imgr.db를 읽지 못했습니다 ({e}). 점수·태그를 지키려고 저장을 멈춥니다. imgr를 끄고 data 폴더를 확인해 주세요 (data\\backup 에 날마다 사본)", "Could not read imgr.db ({e}). Saving is stopped to protect scores and tags. Close imgr and check the data folder (data\\backup has a copy per day)")),
            }
        } else if crate::store::is_stub(&js) {
            return ro(crate::i18n::tr("data\\imgr.db가 없습니다 (db.json은 이미 옮겨진 표시뿐). 저장을 멈춥니다. data\\backup 의 imgr_날짜.db 를 data\\imgr.db 로 복사해 주세요", "data\\imgr.db is missing (db.json only says it was moved). Saving is stopped. Copy a data\\backup\\imgr_<date>.db to data\\imgr.db").into());
        } else if js.exists() || d.join("db.json.bak").exists() {
            let (mut db, note) = match Db::load_json(&d) {
                Ok(x) => x,
                Err(why) => return ro(why),
            };
            match crate::store::convert_json(&mut db, &d) {
                Ok(()) => {
                    db.moved_from_json = Some(db.images.len());
                    (db, note)
                }
                Err(e) => return ro(crate::trf!("db.json을 imgr.db로 옮기지 못했습니다 ({e}). db.json은 그대로 두고 저장을 멈춥니다", "Could not move db.json into imgr.db ({e}). db.json is left as it is and saving is stopped")),
            }
        } else {
            let mut db = Db { version: DB_VERSION, meta_ver: META_VER, ..Default::default() };
            db.need_full = true;
            (db, None)
        };
        if !db.read_only {
            db.reread_old_meta();
        }
        (db, note)
    }

    /// db.json of imgr up to v0.16 (and its .bak if db.json is broken).
    fn load_json(d: &Path) -> Result<(Db, Option<String>), String> {
        let p = d.join("db.json");
        let parse = |path: &Path| -> Result<Db, String> {
            let b = std::fs::read(path).map_err(|e| e.to_string())?;
            serde_json::from_slice::<Db>(&b).map_err(|e| e.to_string())
        };
        match parse(&p) {
            Ok(db) => Ok((db, None)),
            Err(e1) => {
                let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                if p.exists() {
                    let _ = std::fs::copy(&p, d.join(format!("db.broken_{stamp}.json")));
                }
                match parse(&d.join("db.json.bak")) {
                    Ok(db) => Ok((db, Some(crate::trf!("db.json을 읽지 못해 ({e1}) 백업 db.json.bak 으로 열었습니다", "Could not read db.json ({e1}); opened the backup db.json.bak")))),
                    Err(_) => Err(crate::trf!("db.json을 읽지 못했습니다 ({e1}). 점수·태그를 지키려고 저장을 멈춥니다. imgr를 끄고 data 폴더를 확인해 주세요", "Could not read db.json ({e1}). Saving is stopped to protect scores and tags. Close imgr and check the data folder")),
                }
            }
        }
    }

    /// Everything in memory is what the file holds (right after a load).
    pub fn mark_all_saved(&mut self) {
        self.saved_fp = self.images.iter().map(|i| (i.id.clone(), fp_of(i))).collect();
        self.misc_fp = Some(self.misc_fp_now());
        self.need_full = false;
    }

    /// A save failed: the next one writes everything again.
    pub fn forget_saved(&mut self) {
        self.saved_fp.clear();
        self.misc_fp = None;
        self.need_full = true;
    }

    fn misc_fp_now(&self) -> u64 {
        let extra = serde_json::to_string(&self.extra).unwrap_or_default();
        fp_of(&(&self.albums, &self.saved, self.version, self.meta_ver, self.presets_to_albums, extra))
    }

    /// What changed since the last save (UI thread; the writer thread does the disk part).
    pub fn take_delta(&mut self) -> Result<crate::store::Delta, String> {
        if self.read_only {
            return Err(crate::i18n::tr("기록을 읽지 못해서 저장하지 않습니다", "Not saved because the records could not be read").into());
        }
        let mut d = crate::store::Delta { full: self.need_full, ..Default::default() };
        if d.full {
            self.saved_fp.clear();
        }
        let mut now: HashSet<&str> = HashSet::with_capacity(self.images.len());
        for i in &self.images {
            now.insert(i.id.as_str());
            let f = fp_of(i);
            if self.saved_fp.get(&i.id) != Some(&f) {
                d.upsert.push(i.clone());
                self.saved_fp.insert(i.id.clone(), f);
            }
        }
        if self.saved_fp.len() > now.len() {
            d.delete = self.saved_fp.keys().filter(|k| !now.contains(k.as_str())).cloned().collect();
            for k in &d.delete {
                self.saved_fp.remove(k);
            }
        }
        let m = self.misc_fp_now();
        if d.full || self.misc_fp != Some(m) {
            d.misc = Some(crate::store::Misc {
                albums: self.albums.clone(),
                saved: self.saved.clone(),
                meta: vec![
                    ("version", self.version.to_string()),
                    ("meta_ver", self.meta_ver.to_string()),
                    ("presets_to_albums", if self.presets_to_albums { "1" } else { "0" }.to_string()),
                    ("extra", serde_json::to_string(&self.extra).unwrap_or_else(|_| "{}".into())),
                ],
            });
            self.misc_fp = Some(m);
        }
        self.need_full = false;
        Ok(d)
    }

    /// A newer PNG info reader: pictures that came out without checkpoint or prompt are read
    /// again at the next folder scan (scores, tags and albums stay; only the info changes).
    pub fn reread_old_meta(&mut self) -> usize {
        if self.meta_ver >= META_VER {
            return 0;
        }
        let mut n = 0;
        for i in self.images.iter_mut() {
            if i.meta_read && (i.ckpt.is_empty() || i.prompt.is_empty()) {
                i.meta_read = false;
                n += 1;
            }
        }
        self.meta_ver = META_VER;
        n
    }

    pub fn all_tags(&self) -> Vec<(String, usize)> {
        let mut m: HashMap<&str, usize> = HashMap::new();
        for i in &self.images {
            // like every other count in 검색 뷰: what a click on the tag shows (trash is hidden)
            if i.missing || i.trash {
                continue;
            }
            for t in &i.tags {
                *m.entry(t.as_str()).or_default() += 1;
            }
        }
        let mut v: Vec<(String, usize)> = m.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// Merge a finished scan into the records.
    /// `held` = nothing was marked missing: the scan had read errors, or (when
    /// not `sure`) so many pictures vanished at once that it is probably a
    /// drive / folder hiccup. `sure` = a scan the user asked for (start, F5).
    /// Vanished pictures that carry nothing of the user's (no score, tag,
    /// hand-set preset, trash mark) are forgotten outright; the
    /// rest stay as `missing` so their data comes back with the file.
    pub fn merge_scan(&mut self, items: Vec<ScanItem>, scan_errors: usize, sure: bool) -> MergeReport {
        let mut by_id: HashMap<String, usize> = HashMap::new();
        let mut by_path: HashMap<String, usize> = HashMap::new();
        for (i, r) in self.images.iter().enumerate() {
            by_id.insert(r.id.clone(), i);
            if !r.missing {
                by_path.insert(r.path.clone(), i);
            }
        }
        // a file whose bytes changed in place (edited, or re-hashed by a new
        // id version) keeps its record: same path, unknown id
        let item_ids: HashSet<String> = items.iter().map(|it| it.id.clone()).collect();
        let mut rekey: Vec<(usize, String)> = Vec::new();
        for it in &items {
            if by_id.contains_key(&it.id) {
                continue;
            }
            if let Some(&i) = by_path.get(&it.path) {
                if !item_ids.contains(&self.images[i].id) {
                    rekey.push((i, it.id.clone()));
                }
            }
        }
        for (i, id) in rekey {
            by_id.remove(&self.images[i].id);
            self.images[i].id = id.clone();
            self.images[i].meta_read = false;
            by_id.insert(id, i);
        }
        let mut seen: HashSet<usize> = HashSet::new();
        let (mut new, mut moved) = (0, 0);
        let now = chrono::Local::now().timestamp();
        for it in items {
            if let Some(&i) = by_id.get(&it.id) {
                if seen.contains(&i) {
                    continue; // same picture twice in imgs: keep the first
                }
                seen.insert(i);
                let r = &mut self.images[i];
                if r.path != it.path {
                    moved += 1;
                    r.path = it.path.clone();
                    // a renamed upload copy (random name) keeps the preset it came from
                    let p = preset_of(r.name());
                    if !p.is_empty() && !r.preset_set {
                        r.preset = p;
                    }
                }
                r.size = it.size;
                r.mtime = it.mtime;
                r.missing = false;
                if let Some(m) = it.meta {
                    apply_meta(r, m);
                }
            } else {
                let mut r = Img {
                    id: it.id.clone(),
                    path: it.path.clone(),
                    size: it.size,
                    mtime: it.mtime,
                    added: now,
                    ..Default::default()
                };
                r.preset = preset_of(r.name());
                if let Some(m) = it.meta {
                    apply_meta(&mut r, m);
                }
                by_id.insert(r.id.clone(), self.images.len());
                seen.insert(self.images.len());
                self.images.push(r);
                new += 1;
            }
        }
        let would_go = self.images.iter().enumerate().filter(|(i, r)| !seen.contains(i) && !r.missing).count();
        let live = self.images.iter().filter(|r| !r.missing).count();
        if would_go > 0 && (scan_errors > 0 || (!sure && would_go > 50 && would_go * 5 > live)) {
            return MergeReport { new, moved, gone: would_go, held: true, forgot: 0 };
        }
        let mut gone = 0;
        for (i, r) in self.images.iter_mut().enumerate() {
            if !seen.contains(&i) && !r.missing {
                r.missing = true;
                gone += 1;
            }
        }
        let forgot = self.forget_plain_missing();
        MergeReport { new, moved, gone, held: false, forgot }
    }

    /// Drop missing records that hold nothing of the user's. Returns how many.
    pub fn forget_plain_missing(&mut self) -> usize {
        let plain: HashSet<String> = self
            .images
            .iter()
            .filter(|r| r.missing && r.score == 0 && r.tags.is_empty() && r.albums.is_empty() && !r.trash && !r.preset_set)
            .map(|r| r.id.clone())
            .collect();
        if !plain.is_empty() {
            self.images.retain(|r| !(r.missing && plain.contains(&r.id)));
        }
        plain.len()
    }
}

// ---------------------------------------------------------------- albums
impl Db {
    pub fn album(&self, id: u32) -> Option<&Album> {
        self.albums.iter().find(|a| a.id == id)
    }

    /// Albums directly under `parent` (0 = top level), sorted by name.
    pub fn album_children(&self, parent: u32) -> Vec<&Album> {
        let mut v: Vec<&Album> = self.albums.iter().filter(|a| a.parent == parent).collect();
        v.sort_by(|a, b| natural_cmp(&a.name, &b.name).then(a.id.cmp(&b.id)));
        v
    }

    /// The album and everything below it.
    pub fn album_subtree(&self, id: u32) -> HashSet<u32> {
        let mut out = HashSet::from([id]);
        let mut grew = true;
        while grew {
            grew = false;
            for a in &self.albums {
                if out.contains(&a.parent) && out.insert(a.id) {
                    grew = true;
                }
            }
        }
        out
    }

    /// Top-down chain of albums ending at `id` (stops at loops / unknown ids).
    pub fn album_chain(&self, id: u32) -> Vec<&Album> {
        let mut v = Vec::new();
        let mut cur = id;
        while let Some(a) = self.album(cur) {
            if v.iter().any(|x: &&Album| x.id == a.id) {
                break;
            }
            v.push(a);
            cur = a.parent;
        }
        v.reverse();
        v
    }

    /// "A › B › C"
    pub fn album_path(&self, id: u32) -> String {
        self.album_chain(id).iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(" › ")
    }

    /// `name`, or "name 2", "name 3" ... if a sibling already has it.
    pub fn free_album_name(&self, parent: u32, name: &str, except: u32) -> String {
        let taken = |n: &str| self.albums.iter().any(|a| a.parent == parent && a.id != except && a.name.eq_ignore_ascii_case(n));
        if !taken(name) {
            return name.to_string();
        }
        (2..).map(|k| format!("{name} {k}")).find(|n| !taken(n)).unwrap()
    }

    pub fn new_album(&mut self, name: &str, parent: u32) -> u32 {
        let parent = if parent != 0 && self.album(parent).is_none() { 0 } else { parent };
        let id = self.albums.iter().map(|a| a.id).max().unwrap_or(0) + 1;
        let name = self.free_album_name(parent, name.trim(), 0);
        self.albums.push(Album { id, name, parent });
        id
    }

    pub fn rename_album(&mut self, id: u32, name: &str) -> Option<String> {
        let parent = self.album(id)?.parent;
        let name = self.free_album_name(parent, name.trim(), id);
        let a = self.albums.iter_mut().find(|a| a.id == id)?;
        a.name = name.clone();
        Some(name)
    }

    /// Put an album under another one (0 = top level). Refused when the new
    /// parent is the album itself or below it.
    pub fn move_album(&mut self, id: u32, parent: u32) -> bool {
        if self.album(id).is_none() || (parent != 0 && (self.album(parent).is_none() || self.album_subtree(id).contains(&parent))) {
            return false;
        }
        let name = self.album(id).map(|a| a.name.clone()).unwrap_or_default();
        let name = self.free_album_name(parent, &name, id);
        if let Some(a) = self.albums.iter_mut().find(|a| a.id == id) {
            a.parent = parent;
            a.name = name;
        }
        true
    }

    /// Delete an album: its sub albums move up to its parent, its pictures
    /// simply leave it (no picture is deleted). Returns how many pictures left it.
    pub fn delete_album(&mut self, id: u32) -> usize {
        let Some(parent) = self.album(id).map(|a| a.parent) else { return 0 };
        let kids: Vec<u32> = self.albums.iter().filter(|a| a.parent == id).map(|a| a.id).collect();
        self.albums.retain(|a| a.id != id);
        for k in kids {
            self.move_album(k, parent);
        }
        let mut n = 0;
        for im in self.images.iter_mut() {
            let before = im.albums.len();
            im.albums.retain(|&a| a != id);
            n += (before != im.albums.len()) as usize;
            // only the top left (album 0): simply in no album
            if im.albums.iter().all(|&a| a == 0) {
                im.albums.clear();
            }
        }
        n
    }

    /// Once: every preset becomes an album "프리셋 › <name>" holding its pictures.
    /// The preset field itself stays (matrix / compare still read it).
    pub fn presets_into_albums(&mut self) -> usize {
        if self.presets_to_albums {
            return 0;
        }
        self.presets_to_albums = true;
        let mut names: Vec<String> = self.images.iter().filter(|i| !i.preset.is_empty()).map(|i| i.preset.clone()).collect();
        names.sort_by(|a, b| natural_cmp(a, b));
        names.dedup();
        if names.is_empty() {
            return 0;
        }
        let top = self.new_album("프리셋", 0);
        let mut ids: HashMap<String, u32> = HashMap::new();
        for n in &names {
            ids.insert(n.clone(), self.new_album(n, top));
        }
        for im in self.images.iter_mut() {
            if let Some(&a) = ids.get(&im.preset) {
                if !im.albums.contains(&a) {
                    im.albums.push(a);
                }
            }
        }
        names.len()
    }
}

/// Name order with numbers compared as numbers ("a2" < "a10"), ignoring case.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    x.next();
                }
                let mut nb = String::new();
                while let Some(d) = y.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(d);
                    y.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then(ta.cmp(tb));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// What a scan changed (see `Db::merge_scan`).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MergeReport {
    pub new: usize,
    pub moved: usize,
    /// marked missing now (or, when held, would have been)
    pub gone: usize,
    pub held: bool,
    /// missing records removed because they held nothing of the user's
    pub forgot: usize,
}

fn apply_meta(r: &mut Img, m: Meta) {
    r.w = m.w;
    r.h = m.h;
    r.ckpt = m.ckpt;
    r.loras = m.loras;
    r.prompt = m.prompt;
    r.seed = m.seed;
    r.meta_read = true;
}

/// g_<preset>_<digits>.ext -> preset
pub fn preset_of(name: &str) -> String {
    let stem = name.rsplit_once('.').map(|x| x.0).unwrap_or(name);
    if let Some(rest) = stem.strip_prefix("g_") {
        if let Some((p, num)) = rest.rsplit_once('_') {
            if !p.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                return p.to_string();
            }
        }
    }
    String::new()
}

/// Sub folders imports and watch folders never go into: private training
/// sets (data_blind*), kohya work folders, hidden folders.
pub fn skip_dir(p: &Path) -> bool {
    let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    n.starts_with("data_blind") || n.starts_with("_for_kohya") || n.starts_with('.') || n == "$recycle.bin"
}

pub fn is_image(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn mtime_of(m: &std::fs::Metadata) -> u64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Content id: word-wise FNV-style 64-bit hash + CRC32 + size, as hex.
/// Fixed algorithm (unlike std's DefaultHasher) so ids never change between builds.
pub fn hash_file(p: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut crc = crc32fast::Hasher::new();
    let mut carry: Vec<u8> = Vec::with_capacity(8);
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        crc.update(&buf[..n]);
        total += n as u64;
        let mut data: &[u8] = &buf[..n];
        if !carry.is_empty() {
            let need = (8 - carry.len()).min(data.len());
            carry.extend_from_slice(&data[..need]);
            data = &data[need..];
            if carry.len() == 8 {
                h = fnv_word(h, u64::from_le_bytes(carry[..8].try_into().unwrap()));
                carry.clear();
            }
        }
        let mut chunks = data.chunks_exact(8);
        for c in &mut chunks {
            h = fnv_word(h, u64::from_le_bytes(c.try_into().unwrap()));
        }
        carry.extend_from_slice(chunks.remainder());
    }
    for &b in &carry {
        h = fnv_word(h, b as u64);
    }
    Ok(format!("{:016x}{:08x}{:x}", h, crc.finalize(), total))
}

fn fnv_word(h: u64, w: u64) -> u64 {
    (h ^ w).wrapping_mul(0x0000_0100_0000_01b3).rotate_left(23)
}

#[derive(Debug, Default, Clone)]
pub struct Meta {
    pub w: u32,
    pub h: u32,
    pub ckpt: String,
    pub loras: Vec<String>,
    pub prompt: String,
    pub seed: Option<u64>,
}

fn short_model(s: &str) -> String {
    let base = s.rsplit(['/', '\\']).next().unwrap_or(s);
    base.rsplit_once('.').map(|x| x.0).unwrap_or(base).to_string()
}

/// Size + ComfyUI prompt info (PNG text chunk "prompt").
pub fn read_meta(p: &Path) -> Meta {
    let mut m = Meta::default();
    let is_png = p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("png")).unwrap_or(false);
    if is_png {
        if let Ok(f) = std::fs::File::open(p) {
            let dec = png::Decoder::new(std::io::BufReader::new(f));
            if let Ok(reader) = dec.read_info() {
                let info = reader.info();
                m.w = info.width;
                m.h = info.height;
                let mut prompt_json: Option<String> = None;
                for t in &info.uncompressed_latin1_text {
                    if t.keyword == "prompt" {
                        prompt_json = Some(t.text.clone());
                    }
                }
                for t in &info.compressed_latin1_text {
                    if t.keyword == "prompt" {
                        if let Ok(s) = t.get_text() {
                            prompt_json = Some(s);
                        }
                    }
                }
                for t in &info.utf8_text {
                    if t.keyword == "prompt" {
                        if let Ok(s) = t.get_text() {
                            prompt_json = Some(s);
                        }
                    }
                }
                if let Some(js) = prompt_json {
                    parse_comfy(&js, &mut m);
                }
            }
        }
    }
    if m.w == 0 {
        if let Ok((w, h)) = image::image_dimensions(p) {
            m.w = w;
            m.h = h;
        }
    }
    m
}

fn parse_comfy(js: &str, m: &mut Meta) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(js) else { return };
    let Some(obj) = v.as_object() else { return };
    // 1) follow the wires back from the sampler: works the same for every model base
    //    (SDXL CheckpointLoaderSimple, Anima / Flux UNETLoader, GGUF ...) and for prompts
    //    built by string nodes (PrimitiveString -> StringReplace -> CLIPTextEncode)
    graph_meta(obj, m);
    // 2) by node kind, for what is still empty (graphs without wires, unknown samplers)
    // main nodes first ("ckpt", "pos", "sample" are the names run.py gives)
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort_by_key(|k| match k.as_str() {
        "ckpt" | "pos" | "sample" => 0,
        k if k.starts_with("lora") => 1,
        _ => 2,
    });
    for k in keys {
        let n = &obj[k];
        let class = n.get("class_type").and_then(|c| c.as_str()).unwrap_or("");
        let inp = n.get("inputs");
        let s = |key: &str| inp.and_then(|i| i.get(key)).and_then(|x| x.as_str()).map(|x| x.to_string());
        match class {
            "CheckpointLoaderSimple" | "CheckpointLoader" if m.ckpt.is_empty() => {
                if let Some(c) = s("ckpt_name") {
                    m.ckpt = short_model(&c);
                }
            }
            "LoraLoader" | "LoraLoaderModelOnly" if k.starts_with("lora") || m.loras.is_empty() => {
                if let Some(l) = s("lora_name") {
                    let l = short_model(&l);
                    if !m.loras.contains(&l) {
                        m.loras.push(l);
                    }
                }
            }
            "CLIPTextEncode" if m.prompt.is_empty() && (k == "pos" || !k.contains("neg")) => {
                if let Some(t) = s("text") {
                    m.prompt = t;
                }
            }
            "KSampler" | "KSamplerAdvanced" if m.seed.is_none() => {
                let seed = inp.and_then(|i| i.get("seed").or_else(|| i.get("noise_seed"))).and_then(|x| x.as_u64());
                m.seed = seed;
            }
            _ => {}
        }
    }
}

type Graph = serde_json::Map<String, serde_json::Value>;

/// A wire in the ComfyUI API prompt: ["node id", output slot].
fn wire(v: &serde_json::Value) -> Option<String> {
    let a = v.as_array()?;
    if a.len() != 2 || !a[1].is_number() {
        return None;
    }
    match &a[0] {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn node_inputs<'a>(g: &'a Graph, id: &str) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    g.get(id)?.get("inputs")?.as_object()
}

fn node_class<'a>(g: &'a Graph, id: &str) -> &'a str {
    g.get(id).and_then(|n| n.get("class_type")).and_then(|c| c.as_str()).unwrap_or("")
}

/// Sampler, checkpoint, positive prompt and seed by following wires.
fn graph_meta(g: &Graph, m: &mut Meta) {
    // the sampler: a node that takes a model / guider / positive (KSampler, SamplerCustom(Advanced) ...)
    let mut ids: Vec<&String> = g.keys().collect();
    ids.sort_by_key(|k| (k.as_str() != "sample", k.parse::<u64>().unwrap_or(u64::MAX), k.to_string()));
    let Some(sid) = ids.into_iter().find(|id| {
        let c = node_class(g, id);
        let i = node_inputs(g, id);
        c.contains("Sampler") && i.is_some_and(|i| i.contains_key("model") || i.contains_key("guider") || i.contains_key("positive"))
    }) else {
        return;
    };
    let si = node_inputs(g, sid).cloned().unwrap_or_default();
    // a guider (SamplerCustomAdvanced) holds model and conditioning itself
    let guider = si.get("guider").and_then(wire).and_then(|gid| node_inputs(g, &gid).cloned());
    let pick = |keys: &[&str]| -> Option<serde_json::Value> {
        for k in keys {
            if let Some(v) = si.get(*k).or_else(|| guider.as_ref().and_then(|gi| gi.get(*k))) {
                return Some(v.clone());
            }
        }
        None
    };
    if m.ckpt.is_empty() {
        if let Some(name) = pick(&["model"]).and_then(|v| find_model(g, &v)) {
            m.ckpt = short_model(&name);
        }
    }
    if m.prompt.is_empty() {
        if let Some(t) = pick(&["positive", "conditioning", "cond1"]).and_then(|v| find_text(g, &v)) {
            m.prompt = t;
        }
    }
    if m.seed.is_none() {
        m.seed = pick(&["seed", "noise_seed"]).and_then(|v| resolve_num(g, &v, 0)).or_else(|| {
            // SamplerCustomAdvanced: noise <- RandomNoise(noise_seed)
            let nid = pick(&["noise"]).as_ref().and_then(wire)?;
            let ni = node_inputs(g, &nid)?;
            ["noise_seed", "seed"].iter().find_map(|k| ni.get(*k).and_then(|v| resolve_num(g, v, 0)))
        });
    }
}

/// Walk the model wire up to the node that loads it (LoRA nodes pass the model on).
fn find_model(g: &Graph, start: &serde_json::Value) -> Option<String> {
    const NAME_KEYS: [&str; 5] = ["ckpt_name", "unet_name", "model_name", "model_path", "base_ckpt_name"];
    let mut cur = wire(start)?;
    for _ in 0..40 {
        let inp = node_inputs(g, &cur)?;
        if !node_class(g, &cur).to_lowercase().contains("lora") {
            if let Some(n) = NAME_KEYS.iter().find_map(|k| inp.get(*k).and_then(|v| v.as_str())) {
                return Some(n.to_string());
            }
        }
        // go on along the model wire (or anything that looks like one)
        let next = inp.get("model").and_then(wire).or_else(|| {
            inp.iter().filter(|(k, _)| k.contains("model")).find_map(|(_, v)| wire(v))
        })?;
        cur = next;
    }
    None
}

/// Walk the conditioning wire up to the node that holds the prompt text.
fn find_text(g: &Graph, start: &serde_json::Value) -> Option<String> {
    const TEXT_KEYS: [&str; 8] = ["text", "text_g", "prompt", "t5xxl", "clip_l", "text_l", "string", "value"];
    const COND_KEYS: [&str; 6] = ["conditioning", "conditioning_1", "conditioning_to", "positive", "cond1", "conditioning_from"];
    const NOT_TEXT: [&str; 12] = ["clip", "model", "vae", "control_net", "image", "mask", "pixels", "latent", "samples", "style_model", "clip_vision_output", "negative"];
    let mut queue = std::collections::VecDeque::from([wire(start)?]);
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) || seen.len() > 60 {
            continue;
        }
        let Some(inp) = node_inputs(g, &id) else { continue };
        if let Some(t) = TEXT_KEYS.iter().find_map(|k| inp.get(*k).and_then(|v| resolve_text(g, v, 0))) {
            if !t.trim().is_empty() {
                return Some(t);
            }
        }
        for k in COND_KEYS {
            if let Some(w) = inp.get(k).and_then(wire) {
                queue.push_back(w);
            }
        }
        for (k, v) in inp {
            if !COND_KEYS.contains(&k.as_str()) && !NOT_TEXT.contains(&k.as_str()) {
                if let Some(w) = wire(v) {
                    queue.push_back(w);
                }
            }
        }
    }
    None
}

/// A text input: written in place, or wired from string nodes (PrimitiveString,
/// StringReplace, StringConcatenate, wildcard nodes ...).
fn resolve_text(g: &Graph, v: &serde_json::Value, depth: u32) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    if depth > 20 {
        return None;
    }
    let id = wire(v)?;
    let inp = node_inputs(g, &id)?;
    let get = |k: &str| inp.get(k).and_then(|x| resolve_text(g, x, depth + 1));
    // replace: string / text with find -> replace
    if let (Some(base), Some(find)) = (get("string").or_else(|| get("text")), get("find")) {
        let rep = get("replace").or_else(|| get("replacement")).unwrap_or_default();
        return Some(if find.is_empty() { base } else { base.replace(&find, &rep) });
    }
    // join: string_a + delimiter + string_b
    if let (Some(a), Some(b)) = (get("string_a"), get("string_b")) {
        return Some(format!("{a}{}{b}", get("delimiter").unwrap_or_default()));
    }
    ["populated_text", "value", "string", "text", "prompt", "String", "wildcard_text"].iter().find_map(|k| get(k))
}

/// A number input: in place or wired (PrimitiveInt, seed nodes ...).
fn resolve_num(g: &Graph, v: &serde_json::Value, depth: u32) -> Option<u64> {
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    if depth > 10 {
        return None;
    }
    let id = wire(v)?;
    let inp = node_inputs(g, &id)?;
    ["seed", "noise_seed", "value", "int", "number"].iter().find_map(|k| inp.get(*k).and_then(|x| resolve_num(g, x, depth + 1)))
}

pub struct ScanItem {
    pub path: String,
    pub id: String,
    pub size: u64,
    pub mtime: u64,
    pub meta: Option<Meta>,
}

/// Known (path -> (id, size, mtime, meta_read)) so unchanged files are not hashed again.
pub type Known = HashMap<String, (String, u64, u64, bool)>;

pub fn list_files(src: &Path, errors: &mut usize) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![src.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            *errors += 1;
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() && is_image(&p) {
                if let Ok(rel) = p.strip_prefix(src) {
                    let rel = rel.to_string_lossy().replace('\\', "/");
                    out.push((p, rel));
                }
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Scan imgs. `progress(done, total)` is called now and then.
/// Returns the items and the number of read errors (files/folders that could not be read).
pub fn scan(src: &Path, known: &Known, progress: &(dyn Fn(usize, usize) + Sync)) -> (Vec<ScanItem>, usize) {
    let mut errs = 0usize;
    if !src.is_dir() {
        return (Vec::new(), 1);
    }
    let files = list_files(src, &mut errs);
    let errors = std::sync::atomic::AtomicUsize::new(errs);
    let total = files.len();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::with_capacity(total));
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
    std::thread::scope(|sc| {
        for _ in 0..workers {
            sc.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= total {
                        break;
                    }
                    let (p, rel) = &files[i];
                    let Ok(md) = std::fs::metadata(p) else {
                        errors.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        continue;
                    };
                    let (size, mtime) = (md.len(), mtime_of(&md));
                    let item = match known.get(rel) {
                        Some((id, s, t, meta_read)) if *s == size && *t == mtime => ScanItem {
                            path: rel.clone(),
                            id: id.clone(),
                            size,
                            mtime,
                            meta: if *meta_read { None } else { Some(read_meta(p)) },
                        },
                        _ => match hash_file(p) {
                            Ok(id) => ScanItem { path: rel.clone(), id, size, mtime, meta: Some(read_meta(p)) },
                            Err(_) => {
                                // locked / being written: count it so nothing is marked missing this time
                                errors.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                match known.get(rel) {
                                    Some((id, ..)) => ScanItem { path: rel.clone(), id: id.clone(), size, mtime, meta: None },
                                    None => continue,
                                }
                            }
                        },
                    };
                    results.lock().unwrap().push(item);
                    let d = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    if d % 50 == 0 || d == total {
                        progress(d, total);
                    }
                }
            });
        }
    });
    let mut v = results.into_inner().unwrap();
    v.sort_by(|a, b| a.path.cmp(&b.path));
    (v, errors.into_inner())
}

/// The imgs sub folder new pictures go to: the last numbered one with room, else the next number.
pub fn bucket_for_new(src: &Path) -> PathBuf {
    let mut nums: Vec<u32> = std::fs::read_dir(src)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter_map(|e| e.file_name().to_str().and_then(|s| if s.len() == 5 { s.parse().ok() } else { None }))
                .collect()
        })
        .unwrap_or_default();
    nums.sort();
    if let Some(&last) = nums.last() {
        let d = src.join(format!("{last:05}"));
        let n = std::fs::read_dir(&d).map(|rd| rd.flatten().count()).unwrap_or(0);
        if n < per_folder() {
            return d;
        }
        return src.join(format!("{:05}", last + 1));
    }
    src.join("00001")
}

#[derive(Default)]
pub struct ImportReport {
    pub copied: usize,
    /// same content as a picture already in (or earlier in this batch)
    pub skipped_dup: usize,
    /// put in under "name (2).ext" because the name was taken
    pub renamed: usize,
    /// ids of the skipped duplicates (and of files already inside imgs), so an
    /// import into an album can still put them there
    pub dup_ids: Vec<String>,
    pub failed: Vec<String>,
    /// (path relative to imgs, id, size, mtime) of each picture put in, so the
    /// following scan doesn't hash them again.
    pub added: Vec<(String, String, u64, u64)>,
}

/// Copy (or move) pictures into imgs, 2000 per folder. Duplicates are decided
/// by content hash: a picture already in the library, or met earlier in this
/// batch, is skipped. A different picture whose name is taken goes in as
/// "name (2).ext".
pub fn import(paths: &[PathBuf], known_ids: &HashSet<String>, do_move: bool, progress: &dyn Fn(usize, usize)) -> ImportReport {
    import_into(&src_dir(), paths, known_ids, do_move, progress)
}

pub fn import_into(src: &Path, paths: &[PathBuf], known_ids: &HashSet<String>, do_move: bool, progress: &dyn Fn(usize, usize)) -> ImportReport {
    let src = src.to_path_buf();
    let mut files: Vec<PathBuf> = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut st = vec![p.clone()];
            while let Some(d) = st.pop() {
                if let Ok(rd) = std::fs::read_dir(&d) {
                    for e in rd.flatten() {
                        let q = e.path();
                        if q.is_dir() {
                            if !skip_dir(&q) {
                                st.push(q);
                            }
                        } else if is_image(&q) {
                            files.push(q);
                        }
                    }
                }
            }
        } else if is_image(p) {
            files.push(p.clone());
        }
    }
    files.sort();
    let mut rep = ImportReport::default();
    // every name already in imgs (lower case -> files), so a new picture never
    // shares a name with another one anywhere in imgs
    let mut taken: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut list_errs = 0;
    for (p, rel) in list_files(&src, &mut list_errs) {
        let nm = rel.rsplit('/').next().unwrap_or(&rel).to_lowercase();
        taken.entry(nm).or_default().push(p);
    }
    let mut dir = bucket_for_new(&src);
    let mut in_dir = std::fs::read_dir(&dir).map(|rd| rd.flatten().count()).unwrap_or(0);
    let mut ids = known_ids.clone();
    let total = files.len();
    for (k, f) in files.iter().enumerate() {
        progress(k, total);
        if path_inside(f, &src) {
            rep.skipped_dup += 1;
            if let Ok(id) = hash_file(f) {
                rep.dup_ids.push(id);
            }
            continue;
        }
        let id = match hash_file(f) {
            Ok(i) => i,
            Err(e) => {
                rep.failed.push(format!("{}: {e}", f.display()));
                continue;
            }
        };
        if ids.contains(&id) {
            rep.skipped_dup += 1;
            rep.dup_ids.push(id);
            continue;
        }
        if in_dir >= per_folder() {
            dir = bucket_for_new(&src);
            in_dir = std::fs::read_dir(&dir).map(|rd| rd.flatten().count()).unwrap_or(0);
        }
        if let Err(e) = std::fs::create_dir_all(&dir) {
            rep.failed.push(format!("{}: {e}", dir.display()));
            continue;
        }
        let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        // a name already used anywhere in imgs: "name (2).png", "name (3).png" ...
        // (a file there under that name with the same bytes = already in, e.g.
        // put into imgs by hand and not scanned yet)
        let mut cand = name.clone();
        let mut same = false;
        let mut n = 1;
        loop {
            let key = cand.to_lowercase();
            match taken.get(&key) {
                Some(ps) if !ps.is_empty() => {
                    if ps.iter().any(|p| same_bytes(p, f, &id)) {
                        same = true;
                        break;
                    }
                }
                _ if !dir.join(&cand).exists() => break,
                _ => {}
            }
            n += 1;
            let (stem, ext) = name.rsplit_once('.').unwrap_or((&name, ""));
            cand = format!("{stem} ({n}).{ext}");
        }
        if same {
            ids.insert(id.clone());
            rep.skipped_dup += 1;
            rep.dup_ids.push(id);
            continue;
        }
        if n > 1 {
            rep.renamed += 1;
        }
        let dest = dir.join(&cand);
        let r = if do_move && std::fs::rename(f, &dest).is_ok() {
            Ok(())
        } else {
            // copy to a .part file first so a failed copy never leaves a broken picture
            let part = dest.with_extension("part");
            let c = std::fs::copy(f, &part).and_then(|_| {
                if dest.exists() {
                    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, crate::i18n::tr("같은 이름이 생김", "Same name appeared")))
                } else {
                    std::fs::rename(&part, &dest)
                }
            });
            if c.is_err() {
                let _ = std::fs::remove_file(&part);
            }
            c.and_then(|_| if do_move { std::fs::remove_file(f) } else { Ok(()) })
        };
        match r {
            Ok(()) => {
                // keep the original time so "newest first" stays meaningful
                if let Ok(md) = std::fs::metadata(f).or_else(|_| std::fs::metadata(&dest)) {
                    if let Ok(t) = md.modified() {
                        if let Ok(fh) = std::fs::File::options().write(true).open(&dest) {
                            let _ = fh.set_modified(t);
                        }
                    }
                }
                in_dir += 1;
                if let Ok(md) = std::fs::metadata(&dest) {
                    if let Ok(rel) = dest.strip_prefix(&src) {
                        rep.added.push((rel.to_string_lossy().replace('\\', "/"), id.clone(), md.len(), mtime_of(&md)));
                    }
                }
                ids.insert(id);
                taken.entry(cand.to_lowercase()).or_default().push(dest.clone());
                rep.copied += 1;
            }
            Err(e) => rep.failed.push(format!("{}: {e}", f.display())),
        }
    }
    progress(total, total);
    rep
}

/// Does the file at `p` hold the same bytes as `f` (whose hash is `id`)?
fn same_bytes(p: &Path, f: &Path, id: &str) -> bool {
    let (Ok(a), Ok(b)) = (std::fs::metadata(p), std::fs::metadata(f)) else { return false };
    a.len() == b.len() && hash_file(p).map(|h| h == id).unwrap_or(false)
}

/// Characters Windows does not allow in file names.
pub fn bad_name(name: &str) -> Option<&'static str> {
    let n = name.trim();
    if n.is_empty() {
        return Some(crate::i18n::tr("이름이 비었습니다", "The name is empty"));
    }
    if n.chars().any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()) {
        return Some(crate::i18n::tr("\\ / : * ? \" < > | 는 이름에 쓸 수 없습니다", "\\ / : * ? \" < > | cannot be used in a name"));
    }
    if n.ends_with('.') || n.ends_with(' ') {
        return Some(crate::i18n::tr("이름 끝에 점이나 빈칸은 안 됩니다", "A name cannot end with a dot or space"));
    }
    None
}

/// Rename a picture inside its imgs folder. Returns the new relative path.
pub fn rename_in_place(rel: &str, new_name: &str) -> Result<String, String> {
    if let Some(e) = bad_name(new_name) {
        return Err(e.to_string());
    }
    let src = src_dir();
    let from = src.join(rel.replace('/', "\\"));
    let dir = from.parent().ok_or(crate::i18n::tr("폴더를 알 수 없음", "Folder unknown"))?.to_path_buf();
    let to = dir.join(new_name.trim());
    if to == from {
        return Ok(rel.to_string());
    }
    // same name with different case is allowed (Windows treats it as the same file)
    if to.exists() && to.to_string_lossy().to_lowercase() != from.to_string_lossy().to_lowercase() {
        return Err(crate::trf!("같은 이름이 이미 있습니다: {}", "That name already exists: {}", new_name.trim()));
    }
    std::fs::rename(&from, &to).map_err(|e| e.to_string())?;
    Ok(to.strip_prefix(&src).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default())
}

/// Upload-style name: 7 letters of time since 2026-01-01 (base 26, sorts by time) + 4 random [0-9a-z].
pub fn upload_name(seq: u64) -> String {
    const EPOCH: i64 = 1767193200;
    let mut t = (chrono::Local::now().timestamp() - EPOCH).max(0) as u64 + seq;
    let mut head = [b'a'; 7];
    for k in (0..7).rev() {
        head[k] = b'a' + (t % 26) as u8;
        t /= 26;
    }
    let alnum = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let tail: String = (0..4).map(|_| alnum[fastrand::usize(..alnum.len())] as char).collect();
    format!("{}{}", String::from_utf8_lossy(&head), tail)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert!(bad_name("a:b.png").is_some());
        assert!(bad_name("ok name.png").is_none());
        assert!(bad_name("x.").is_some());
        let u = upload_name(0);
        assert_eq!(u.len(), 11);
        assert!(u[..7].chars().all(|c| c.is_ascii_lowercase()));
    }
    #[test]
    fn preset_names() {
        assert_eq!(preset_of("g_animeA_003640.png"), "animeA");
        assert_eq!(preset_of("g_sa92eyekJ_003477.png"), "sa92eyekJ");
        assert_eq!(preset_of("aciidlh7w3j.jpg"), "");
        assert_eq!(preset_of("g_x_y.png"), "");
    }
    #[test]
    fn comfy_prompt() {
        let js = r#"{"ckpt":{"class_type":"CheckpointLoaderSimple","inputs":{"ckpt_name":"OneObsession24.safetensors"}},
          "lora0":{"class_type":"LoraLoader","inputs":{"lora_name":"style_pastel1.safetensors"}},
          "pos":{"class_type":"CLIPTextEncode","inputs":{"text":"1girl, solo"}},
          "neg":{"class_type":"CLIPTextEncode","inputs":{"text":"bad"}},
          "sample":{"class_type":"KSampler","inputs":{"seed":123}}}"#;
        let mut m = Meta::default();
        parse_comfy(js, &mut m);
        assert_eq!(m.ckpt, "OneObsession24");
        assert_eq!(m.loras, vec!["style_pastel1".to_string()]);
        assert_eq!(m.prompt, "1girl, solo");
        assert_eq!(m.seed, Some(123));
    }
    #[test]
    fn comfy_wired_anima() {
        // Anima: UNETLoader -> LoraLoaderModelOnly -> KSampler, prompt built by string nodes
        let js = r#"{"unet":{"class_type":"UNETLoader","inputs":{"unet_name":"One obsession_animaV5turbo.safetensors"}},
          "lora":{"class_type":"LoraLoaderModelOnly","inputs":{"lora_name":"anima_onePns2.safetensors","model":["unet",0]}},
          "naia_p":{"class_type":"PrimitiveString","inputs":{"value":"1girl, solo"}},
          "naia_n":{"class_type":"PrimitiveString","inputs":{"value":"worst quality"}},
          "naia_rep1":{"class_type":"StringReplace","inputs":{"string":"anima_onePns, masterpiece, __NAIAPOS__","find":"__NAIAPOS__","replace":["naia_p",0]}},
          "pos":{"class_type":"CLIPTextEncode","inputs":{"text":["naia_rep1",0],"clip":["clip",0]}},
          "neg":{"class_type":"CLIPTextEncode","inputs":{"text":["naia_n",0],"clip":["clip",0]}},
          "seedn":{"class_type":"PrimitiveInt","inputs":{"value":85731387922058}},
          "sample":{"class_type":"KSampler","inputs":{"model":["lora",0],"positive":["pos",0],"negative":["neg",0],"seed":["seedn",0]}}}"#;
        let mut m = Meta::default();
        parse_comfy(js, &mut m);
        assert_eq!(m.ckpt, "One obsession_animaV5turbo");
        assert_eq!(m.loras, vec!["anima_onePns2".to_string()]);
        assert_eq!(m.prompt, "anima_onePns, masterpiece, 1girl, solo");
        assert_eq!(m.seed, Some(85731387922058));
    }
    #[test]
    fn comfy_wired_flux_custom_sampler() {
        // Flux style: GGUF loader, CFGGuider, RandomNoise, Flux text encoder, numbered node ids
        let js = r#"{"10":{"class_type":"UnetLoaderGGUF","inputs":{"unet_name":"flux1-dev-Q8_0.gguf"}},
          "11":{"class_type":"LoraLoader","inputs":{"lora_name":"style_x.safetensors","model":["10",0],"clip":["12",0]}},
          "6":{"class_type":"CLIPTextEncodeFlux","inputs":{"clip_l":"a cat","t5xxl":"a cat on a sofa","guidance":3.5,"clip":["11",1]}},
          "22":{"class_type":"CFGGuider","inputs":{"model":["11",0],"positive":["6",0],"negative":["7",0],"cfg":1}},
          "25":{"class_type":"RandomNoise","inputs":{"noise_seed":42}},
          "16":{"class_type":"KSamplerSelect","inputs":{"sampler_name":"euler"}},
          "13":{"class_type":"SamplerCustomAdvanced","inputs":{"noise":["25",0],"guider":["22",0],"sampler":["16",0]}}}"#;
        let mut m = Meta::default();
        parse_comfy(js, &mut m);
        assert_eq!(m.ckpt, "flux1-dev-Q8_0");
        assert_eq!(m.loras, vec!["style_x".to_string()]);
        assert_eq!(m.prompt, "a cat on a sofa");
        assert_eq!(m.seed, Some(42));
    }
    #[test]
    fn reread_once() {
        let mut db = Db::default();
        let it = |p: &str, id: &str| ScanItem { path: p.into(), id: id.into(), size: 1, mtime: 1, meta: None };
        db.merge_scan(vec![it("00001/a.png", "h1"), it("00001/b.png", "h2")], 0, true);
        for i in db.images.iter_mut() {
            i.meta_read = true;
        }
        db.images[0].ckpt = "x".into();
        db.images[0].prompt = "p".into();
        assert_eq!(db.reread_old_meta(), 1); // only the empty one
        assert!(db.images[0].meta_read && !db.images[1].meta_read);
        assert_eq!(db.reread_old_meta(), 0); // once
    }
    #[test]
    fn merge_keeps_tags_on_move() {
        let mut db = Db::default();
        let it = |p: &str, id: &str| ScanItem { path: p.into(), id: id.into(), size: 1, mtime: 1, meta: None };
        db.merge_scan(vec![it("00001/a.png", "h1"), it("00001/b.png", "h2"), it("00001/c.png", "h3")], 0, true);
        db.images[0].tags.push("pixiv".into());
        db.images[2].score = 4;
        let r = db.merge_scan(vec![it("00002/a2.png", "h1")], 0, true);
        assert_eq!((r.new, r.moved, r.gone, r.forgot), (0, 1, 2, 1));
        assert_eq!(db.images[0].path, "00002/a2.png");
        assert_eq!(db.images[0].tags, vec!["pixiv".to_string()]);
        // b (nothing of the user's) is forgotten, c (scored) stays as missing
        assert_eq!(db.images.len(), 2);
        assert!(db.images[1].id == "h3" && db.images[1].missing);
        // c comes back with its score
        let r = db.merge_scan(vec![it("00002/a2.png", "h1"), it("00001/c.png", "h3")], 0, true);
        assert_eq!((r.new, r.gone), (0, 0));
        assert!(!db.images[1].missing && db.images[1].score == 4);
    }

    #[test]
    fn hash_is_stable_and_path_keeps_record() {
        let d = std::env::temp_dir().join("imgr_hash_test.bin");
        std::fs::write(&d, b"hello imgr 0123456789abcdef!").unwrap();
        let h = hash_file(&d).unwrap();
        assert_eq!(h, hash_file(&d).unwrap());

        // pinned: ids must never change between builds
        assert_eq!(h, "8f178f78c82e3af0466762121c");
        let mut db = Db::default();
        let it = |p: &str, id: &str| ScanItem { path: p.into(), id: id.into(), size: 1, mtime: 1, meta: None };
        db.merge_scan(vec![it("00001/a.png", "old")], 0, true);
        db.images[0].score = 5;
        // same path, new bytes/id: the record (score) follows
        let r = db.merge_scan(vec![it("00001/a.png", "new")], 0, true);
        assert_eq!((r.new, r.gone, r.held), (0, 0, false));
        assert_eq!(db.images[0].id, "new");
        assert_eq!(db.images[0].score, 5);
    }

    #[test]
    fn mass_missing_is_held_unless_sure() {
        let mut db = Db::default();
        let it = |k: usize| ScanItem { path: format!("00001/{k}.png"), id: format!("h{k}"), size: 1, mtime: 1, meta: None };
        db.merge_scan((0..100).map(it).collect(), 0, true);
        for i in db.images.iter_mut() {
            i.score = 1; // keep records around as missing, to count them
        }
        // background scan: a mass vanish is held
        let r = db.merge_scan((0..10).map(it).collect(), 0, false);
        assert!(r.held && r.gone == 90);
        assert!(db.images.iter().all(|i| !i.missing));
        // read errors: held even when sure
        assert!(db.merge_scan((0..99).map(it).collect(), 1, true).held);
        // small vanish in the background goes through
        let r = db.merge_scan((0..97).map(it).collect(), 0, false);
        assert!(!r.held && r.gone == 3);
        // start / F5: the user deleted them, believe the folder (even an empty one)
        let r = db.merge_scan(Vec::new(), 0, true);
        assert!(!r.held && r.gone == 97);
        assert!(db.images.iter().all(|i| i.missing));
    }

    #[test]
    fn plain_missing_forgotten() {
        let mut db = Db::default();
        let it = |k: usize| ScanItem { path: format!("00001/{k}.png"), id: format!("h{k}"), size: 1, mtime: 1, meta: None };
        db.merge_scan((0..5).map(it).collect(), 0, true);
        db.images[1].trash = true;
        db.images[2].preset_set = true;
        let r = db.merge_scan(Vec::new(), 0, true);
        assert_eq!((r.gone, r.forgot), (5, 3));
        let left: Vec<&str> = db.images.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(left, vec!["h1", "h2"]);
    }

    #[test]
    fn import_dedups_by_hash_and_renames_clashes() {
        let base = std::env::temp_dir().join(format!("imgr_import_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (src, a, b) = (base.join("imgsrc"), base.join("a"), base.join("b"));
        for d in [&src.join("00001"), &a, &b] {
            std::fs::create_dir_all(d).unwrap();
        }
        // already in imgsrc (put there by hand, not scanned): x.png
        std::fs::write(src.join("00001/x.png"), b"picture one").unwrap();
        std::fs::write(a.join("x.png"), b"picture one").unwrap(); // same bytes, same name -> skip
        std::fs::write(b.join("x.png"), b"picture two").unwrap(); // other bytes, same name -> x (2)
        std::fs::write(a.join("y.png"), b"picture three").unwrap();
        std::fs::write(b.join("y.png"), b"picture three").unwrap(); // same bytes twice in one batch -> once
        std::fs::write(a.join("Z.PNG"), b"picture four").unwrap();
        std::fs::write(b.join("z.png"), b"picture five").unwrap(); // name clash ignoring case -> z (2)
        let r = import_into(&src, &[a.clone(), b.clone()], &HashSet::new(), false, &|_, _| {});
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert_eq!((r.copied, r.skipped_dup, r.renamed), (4, 2, 2));
        let mut names: Vec<String> = list_files(&src, &mut 0).into_iter().map(|x| x.1).collect();
        names.sort();
        assert_eq!(names, vec!["00001/Z.PNG", "00001/x (2).png", "00001/x.png", "00001/y.png", "00001/z (2).png"]);
        assert_eq!(std::fs::read(src.join("00001/x (2).png")).unwrap(), b"picture two");
        // a second import of the same files: all known by hash now
        let ids: HashSet<String> = r.added.iter().map(|x| x.1.clone()).collect();
        let r2 = import_into(&src, &[a, b], &ids, false, &|_, _| {});
        assert_eq!((r2.copied, r2.skipped_dup), (0, 6));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn albums_tree_rules() {
        let mut db = Db::default();
        let a = db.new_album("여행", 0);
        let b = db.new_album("여행", 0); // same name beside it -> "여행 2"
        assert_eq!(db.album(b).unwrap().name, "여행 2");
        let c = db.new_album("바다", a);
        let d = db.new_album("밤", c);
        assert_eq!(db.album_path(d), "여행 › 바다 › 밤");
        // no loops
        assert!(!db.move_album(a, d) && !db.move_album(a, a));
        assert!(db.move_album(d, 0));
        assert_eq!(db.album(d).unwrap().parent, 0);
        assert!(db.move_album(d, c));
        // rename keeps names apart among siblings, case-insensitive
        assert_eq!(db.rename_album(b, "여행").as_deref(), Some("여행 2"));
        assert_eq!(db.rename_album(b, "Sea").as_deref(), Some("Sea"));
        // one picture in two albums; deleting an album only drops the membership
        db.images.push(Img { id: "p1".into(), albums: vec![c, b], ..Default::default() });
        assert_eq!(db.delete_album(c), 1);
        assert_eq!(db.images[0].albums, vec![b]);
        // its sub album moved up to its parent
        assert_eq!(db.album(d).unwrap().parent, a);
        // a picture only in an album is not "plain": it stays as missing
        db.images[0].missing = true;
        assert_eq!(db.forget_plain_missing(), 0);
    }

    #[test]
    fn presets_become_albums_once() {
        let mut db = Db::default();
        for (id, p) in [("a", "animeA"), ("b", "photoB"), ("c", "animeA"), ("d", "")] {
            db.images.push(Img { id: id.into(), preset: p.into(), ..Default::default() });
        }
        assert_eq!(db.presets_into_albums(), 2);
        let top = db.album_children(0);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].name, "프리셋");
        let kids: Vec<String> = db.album_children(top[0].id).iter().map(|a| a.name.clone()).collect();
        assert_eq!(kids, vec!["animeA", "photoB"]);
        assert!(db.images[0].albums.len() == 1 && db.images[0].albums == db.images[2].albums);
        assert!(db.images[3].albums.is_empty());
        assert_eq!(db.presets_into_albums(), 0); // only once
        assert_eq!(db.albums.len(), 3);
    }

    #[test]
    fn tag_counts_skip_trash() {
        let mut db = Db::default();
        for (k, trash) in [false, false, true].into_iter().enumerate() {
            db.images.push(Img { id: format!("t{k}"), tags: vec!["swim".into()], trash, ..Default::default() });
        }
        assert_eq!(db.all_tags(), vec![("swim".to_string(), 2)]);
    }

    #[test]
    fn natural_name_order() {
        let mut v = vec!["a10", "a2", "B1", "a1"];
        v.sort_by(|x, y| natural_cmp(x, y));
        assert_eq!(v, vec!["a1", "a2", "a10", "B1"]);
    }

    #[test]
    fn old_imgsrc_is_renamed_to_imgs() {
        let base = std::env::temp_dir().join(format!("imgr_src_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("imgsrc/00001")).unwrap();
        std::fs::write(base.join("imgsrc/00001/a.png"), b"x").unwrap();
        assert_eq!(pick_src(&base), base.join("imgs"));
        assert!(base.join("imgs/00001/a.png").exists() && !base.join("imgsrc").exists());
        // both there: imgs wins, imgsrc untouched
        std::fs::create_dir_all(base.join("imgsrc")).unwrap();
        assert_eq!(pick_src(&base), base.join("imgs"));
        assert!(base.join("imgsrc").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn inside_is_case_insensitive() {
        assert!(path_inside(Path::new("D:/X/imgsrc/00001/a.png"), Path::new("d:/x/IMGSRC")));
        assert!(!path_inside(Path::new("d:/x/imgsrc2/a.png"), Path::new("d:/x/imgsrc")));
        assert!(skip_dir(Path::new("x/data_blind3")) && !skip_dir(Path::new("x/_test_out")));
    }
}
