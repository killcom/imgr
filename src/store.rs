//! The library on disk: one SQLite file, data/imgr.db (since v0.20; db.json before).
//!
//! The whole library still lives in memory (`Db`); this file is only where it is kept.
//! A save writes only the pictures that changed since the last save, in one
//! transaction, so a crash leaves either the old or the new state, never half.
//! Tags and album memberships are their own tables (one row each), so the file
//! can also be read with any SQLite tool.

use crate::db::{Album, Db, Img, SavedFilter};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

pub const FILE: &str = "imgr.db";
/// Layout of the tables below. A newer layout than this exe knows is opened read-only.
pub const SCHEMA: i64 = 1;
/// First line of the db.json left behind after the move, so older imgr versions
/// cannot read it (they then stop saving) and this one knows the move happened.
pub const STUB_MARK: &str = "IMGR-MOVED-TO-SQLITE";

const TABLES: &str = "
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS images(
  id TEXT PRIMARY KEY, path TEXT NOT NULL, size INTEGER NOT NULL, mtime INTEGER NOT NULL,
  w INTEGER NOT NULL, h INTEGER NOT NULL, added INTEGER NOT NULL, score INTEGER NOT NULL,
  trash INTEGER NOT NULL, missing INTEGER NOT NULL, preset TEXT NOT NULL, preset_set INTEGER NOT NULL,
  ckpt TEXT NOT NULL, loras TEXT NOT NULL, prompt TEXT NOT NULL, seed INTEGER, meta_read INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS image_tags(img TEXT NOT NULL, pos INTEGER NOT NULL, tag TEXT NOT NULL,
  PRIMARY KEY(img, pos)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS image_tags_tag ON image_tags(tag);
CREATE TABLE IF NOT EXISTS image_albums(img TEXT NOT NULL, pos INTEGER NOT NULL, album INTEGER NOT NULL,
  PRIMARY KEY(img, pos)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS image_albums_album ON image_albums(album);
CREATE TABLE IF NOT EXISTS albums(id INTEGER PRIMARY KEY, name TEXT NOT NULL, parent INTEGER NOT NULL, pos INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS saved_filters(pos INTEGER PRIMARY KEY, name TEXT NOT NULL, query TEXT NOT NULL);
";

/// What one save writes.
#[derive(Default)]
pub struct Delta {
    /// clear every table first (first write, or after a failed save)
    pub full: bool,
    pub upsert: Vec<Img>,
    pub delete: Vec<String>,
    /// albums, saved filters and the small values: written whole when set
    pub misc: Option<Misc>,
}

pub struct Misc {
    pub albums: Vec<Album>,
    pub saved: Vec<SavedFilter>,
    pub meta: Vec<(&'static str, String)>,
}

impl Delta {
    pub fn is_empty(&self) -> bool {
        !self.full && self.upsert.is_empty() && self.delete.is_empty() && self.misc.is_none()
    }
}

pub fn path() -> PathBuf {
    crate::db::data_dir().join(FILE)
}

pub fn open(p: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(p)?;
    c.busy_timeout(std::time::Duration::from_secs(10))?;
    // WAL: a save never blocks a reader and a crash mid-save loses nothing older
    c.pragma_update(None, "journal_mode", "WAL")?;
    c.pragma_update(None, "synchronous", "NORMAL")?;
    c.execute_batch(TABLES)?;
    Ok(c)
}

fn b(v: bool) -> i64 {
    v as i64
}

/// Read the whole library. `Err` = the file is there but cannot be used.
pub fn load(c: &Connection) -> Result<Db, String> {
    let e = |e: rusqlite::Error| e.to_string();
    let meta = |k: &str| -> Result<Option<String>, String> {
        c.query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get::<_, String>(0)).optional().map_err(e)
    };
    let schema: i64 = meta("schema")?.and_then(|s| s.parse().ok()).unwrap_or(SCHEMA);
    let mut db = Db::default();
    if schema > SCHEMA {
        db.read_only = true;
    }
    db.version = meta("version")?.and_then(|s| s.parse().ok()).unwrap_or(crate::db::DB_VERSION);
    db.meta_ver = meta("meta_ver")?.and_then(|s| s.parse().ok()).unwrap_or(crate::db::META_VER);
    db.presets_to_albums = meta("presets_to_albums")?.is_some_and(|s| s == "1");
    if let Some(x) = meta("extra")? {
        db.extra = serde_json::from_str(&x).map_err(|e| e.to_string())?;
    }
    let mut tags: std::collections::HashMap<String, Vec<String>> = Default::default();
    {
        let mut st = c.prepare("SELECT img, tag FROM image_tags ORDER BY img, pos").map_err(e)?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(e)?;
        for r in rows {
            let (i, t) = r.map_err(e)?;
            tags.entry(i).or_default().push(t);
        }
    }
    let mut albs: std::collections::HashMap<String, Vec<u32>> = Default::default();
    {
        let mut st = c.prepare("SELECT img, album FROM image_albums ORDER BY img, pos").map_err(e)?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(e)?;
        for r in rows {
            let (i, a) = r.map_err(e)?;
            albs.entry(i).or_default().push(a as u32);
        }
    }
    let mut st = c
        .prepare(
            "SELECT id, path, size, mtime, w, h, added, score, trash, missing, preset, preset_set,
                    ckpt, loras, prompt, seed, meta_read FROM images ORDER BY rowid",
        )
        .map_err(e)?;
    let rows = st
        .query_map([], |r| {
            Ok(Img {
                id: r.get(0)?,
                path: r.get(1)?,
                size: r.get::<_, i64>(2)? as u64,
                mtime: r.get::<_, i64>(3)? as u64,
                w: r.get::<_, i64>(4)? as u32,
                h: r.get::<_, i64>(5)? as u32,
                added: r.get(6)?,
                score: r.get::<_, i64>(7)? as u8,
                trash: r.get::<_, i64>(8)? != 0,
                missing: r.get::<_, i64>(9)? != 0,
                preset: r.get(10)?,
                preset_set: r.get::<_, i64>(11)? != 0,
                ckpt: r.get(12)?,
                loras: serde_json::from_str(&r.get::<_, String>(13)?).unwrap_or_default(),
                prompt: r.get(14)?,
                seed: r.get::<_, Option<i64>>(15)?.map(|s| s as u64),
                meta_read: r.get::<_, i64>(16)? != 0,
                tags: Vec::new(),
                albums: Vec::new(),
            })
        })
        .map_err(e)?;
    for r in rows {
        let mut i = r.map_err(e)?;
        i.tags = tags.remove(&i.id).unwrap_or_default();
        i.albums = albs.remove(&i.id).unwrap_or_default();
        db.images.push(i);
    }
    let mut st = c.prepare("SELECT id, name, parent FROM albums ORDER BY pos").map_err(e)?;
    for r in st
        .query_map([], |r| Ok(Album { id: r.get::<_, i64>(0)? as u32, name: r.get(1)?, parent: r.get::<_, i64>(2)? as u32 }))
        .map_err(e)?
    {
        db.albums.push(r.map_err(e)?);
    }
    let mut st = c.prepare("SELECT name, query FROM saved_filters ORDER BY pos").map_err(e)?;
    for r in st.query_map([], |r| Ok(SavedFilter { name: r.get(0)?, query: r.get(1)? })).map_err(e)? {
        db.saved.push(r.map_err(e)?);
    }
    Ok(db)
}

/// Write one save in a single transaction.
pub fn apply(c: &mut Connection, d: &Delta) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    if d.full {
        tx.execute_batch("DELETE FROM images; DELETE FROM image_tags; DELETE FROM image_albums;")?;
    }
    {
        let mut up = tx.prepare_cached(
            "INSERT INTO images(id, path, size, mtime, w, h, added, score, trash, missing, preset, preset_set,
                                ckpt, loras, prompt, seed, meta_read)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
             ON CONFLICT(id) DO UPDATE SET path = excluded.path, size = excluded.size, mtime = excluded.mtime,
               w = excluded.w, h = excluded.h, added = excluded.added, score = excluded.score, trash = excluded.trash,
               missing = excluded.missing, preset = excluded.preset, preset_set = excluded.preset_set,
               ckpt = excluded.ckpt, loras = excluded.loras, prompt = excluded.prompt, seed = excluded.seed,
               meta_read = excluded.meta_read",
        )?;
        let mut del_t = tx.prepare_cached("DELETE FROM image_tags WHERE img = ?1")?;
        let mut del_a = tx.prepare_cached("DELETE FROM image_albums WHERE img = ?1")?;
        let mut ins_t = tx.prepare_cached("INSERT INTO image_tags(img, pos, tag) VALUES(?1, ?2, ?3)")?;
        let mut ins_a = tx.prepare_cached("INSERT INTO image_albums(img, pos, album) VALUES(?1, ?2, ?3)")?;
        let mut del_i = tx.prepare_cached("DELETE FROM images WHERE id = ?1")?;
        for i in &d.upsert {
            up.execute(params![
                i.id,
                i.path,
                i.size as i64,
                i.mtime as i64,
                i.w as i64,
                i.h as i64,
                i.added,
                i.score as i64,
                b(i.trash),
                b(i.missing),
                i.preset,
                b(i.preset_set),
                i.ckpt,
                serde_json::to_string(&i.loras).unwrap_or_else(|_| "[]".into()),
                i.prompt,
                i.seed.map(|s| s as i64),
                b(i.meta_read),
            ])?;
            if !d.full {
                del_t.execute([&i.id])?;
                del_a.execute([&i.id])?;
            }
            for (k, t) in i.tags.iter().enumerate() {
                ins_t.execute(params![i.id, k as i64, t])?;
            }
            for (k, a) in i.albums.iter().enumerate() {
                ins_a.execute(params![i.id, k as i64, *a as i64])?;
            }
        }
        for id in &d.delete {
            del_i.execute([id])?;
            del_t.execute([id])?;
            del_a.execute([id])?;
        }
    }
    if let Some(m) = &d.misc {
        tx.execute_batch("DELETE FROM albums; DELETE FROM saved_filters;")?;
        {
            let mut st = tx.prepare_cached("INSERT INTO albums(id, name, parent, pos) VALUES(?1, ?2, ?3, ?4)")?;
            for (k, a) in m.albums.iter().enumerate() {
                st.execute(params![a.id as i64, a.name, a.parent as i64, k as i64])?;
            }
            let mut st = tx.prepare_cached("INSERT INTO saved_filters(pos, name, query) VALUES(?1, ?2, ?3)")?;
            for (k, s) in m.saved.iter().enumerate() {
                st.execute(params![k as i64, s.name, s.query])?;
            }
            let mut st = tx.prepare_cached("INSERT OR REPLACE INTO meta(key, value) VALUES(?1, ?2)")?;
            st.execute(params!["schema", SCHEMA.to_string()])?;
            for (k, v) in &m.meta {
                st.execute(params![k, v])?;
            }
        }
    }
    tx.commit()
}

/// One copy per day in data/backup (imgr_YYYYMMDD.db), the last 14 kept.
/// `VACUUM INTO` makes a whole, consistent copy while the file is open.
pub fn daily_backup(c: &Connection) {
    let bdir = crate::db::data_dir().join("backup");
    let today = bdir.join(format!("imgr_{}.db", chrono::Local::now().format("%Y%m%d")));
    if today.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&bdir);
    if c.execute("VACUUM INTO ?1", [today.to_string_lossy()]).is_err() {
        let _ = std::fs::remove_file(&today);
        return;
    }
    if let Ok(rd) = std::fs::read_dir(&bdir) {
        let mut v: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "db") && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("imgr_")))
            .collect();
        v.sort();
        while v.len() > 14 {
            let _ = std::fs::remove_file(v.remove(0));
        }
    }
}

/// The one-time move db.json -> imgr.db. Writes into imgr.db.tmp, reads it back and
/// compares every field with what db.json held; only then the file gets its real
/// name, db.json is renamed to db.json.v016 (kept) and a stub db.json is written
/// that older imgr versions cannot open (so they never save over the library).
pub fn convert_json(db: &mut Db, dir: &Path) -> Result<(), String> {
    let tmp = dir.join(format!("{FILE}.tmp"));
    for x in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(dir.join(format!("{FILE}.tmp{x}")));
    }
    {
        let mut c = open(&tmp).map_err(|e| e.to_string())?;
        let d = db.take_delta()?;
        apply(&mut c, &d).map_err(|e| e.to_string())?;
        let back = load(&c)?;
        if let Some(why) = same_db(db, &back) {
            drop(c);
            let _ = std::fs::remove_file(&tmp);
            db.forget_saved();
            return Err(why);
        }
        c.pragma_update(None, "journal_mode", "DELETE").map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| e.to_string())?;
    let js = dir.join("db.json");
    let _ = std::fs::rename(&js, dir.join("db.json.v016"));
    let _ = std::fs::rename(dir.join("db.json.bak"), dir.join("db.json.bak.v016"));
    let _ = std::fs::write(
        &js,
        format!(
            "{STUB_MARK}\r\nimgr v0.20 부터 기록은 data\\imgr.db 에 있습니다. 예전 기록: db.json.v016\r\n\
             Since imgr v0.20 the records are in data\\imgr.db. The old file: db.json.v016\r\n"
        ),
    );
    Ok(())
}

/// None = the two hold the same library; Some(what differs) otherwise.
pub fn same_db(a: &Db, b: &Db) -> Option<String> {
    if a.images.len() != b.images.len() {
        return Some(format!("pictures {} != {}", a.images.len(), b.images.len()));
    }
    if let Some(k) = (0..a.images.len()).find(|&k| a.images[k] != b.images[k]) {
        return Some(format!("picture {} differs", a.images[k].id));
    }
    if a.albums != b.albums {
        return Some("albums differ".into());
    }
    if a.saved != b.saved {
        return Some("saved filters differ".into());
    }
    if (a.version, a.meta_ver, a.presets_to_albums) != (b.version, b.meta_ver, b.presets_to_albums) || a.extra != b.extra {
        return Some("settings differ".into());
    }
    None
}

pub fn is_stub(p: &Path) -> bool {
    std::fs::read(p).map(|b| b.starts_with(STUB_MARK.as_bytes())).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(id: &str, n: u64) -> Img {
        Img {
            id: id.into(),
            path: format!("00001/g_{id}.png"),
            size: n,
            mtime: u64::MAX - n,
            w: 832,
            h: 1216,
            added: 1_700_000_000 + n as i64,
            score: (n % 7) as u8,
            tags: vec!["b".into(), "a".into()],
            seed: Some(u64::MAX - 1),
            loras: vec!["x:0.8".into()],
            prompt: "1girl, 한글".into(),
            albums: vec![3, 1],
            meta_read: true,
            ..Default::default()
        }
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("imgr_store_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn round_trip_and_only_changes_written() {
        let d = dir("rt");
        let mut db = Db::default();
        (db.version, db.meta_ver) = (2, 2);
        db.forget_saved();
        db.images = (0..50).map(|n| img(&format!("id{n:03}"), n)).collect();
        db.albums = vec![Album { id: 3, name: "여행".into(), parent: 0 }, Album { id: 1, name: "a".into(), parent: 3 }];
        db.saved = vec![SavedFilter { name: "f".into(), query: "#a".into() }];
        db.extra.insert("lib".into(), serde_json::json!({"k": 1}));
        let mut c = open(&d.join(FILE)).unwrap();
        let first = db.take_delta().unwrap();
        assert!(first.full && first.upsert.len() == 50 && first.misc.is_some());
        apply(&mut c, &first).unwrap();
        assert!(db.take_delta().unwrap().is_empty(), "nothing changed -> nothing written");
        db.images[7].score = 6;
        db.images[7].tags.push("new".into());
        db.images.remove(3);
        db.images.push(img("zzz", 99));
        let d2 = db.take_delta().unwrap();
        assert_eq!(d2.upsert.len(), 2);
        assert_eq!(d2.delete, vec!["id003".to_string()]);
        assert!(d2.misc.is_none());
        apply(&mut c, &d2).unwrap();
        db.albums[1].name = "b".into();
        let d3 = db.take_delta().unwrap();
        assert!(d3.upsert.is_empty() && d3.misc.is_some());
        apply(&mut c, &d3).unwrap();
        let back = load(&c).unwrap();
        assert_eq!(same_db(&db, &back), None);
        // a failed save: the next one rewrites everything
        db.forget_saved();
        let d4 = db.take_delta().unwrap();
        assert!(d4.full && d4.upsert.len() == 50);
        apply(&mut c, &d4).unwrap();
        assert_eq!(same_db(&db, &load(&c).unwrap()), None);
        drop(c);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn json_moved_once_and_old_file_blocked() {
        let d = dir("conv");
        let mut db = Db::default();
        (db.version, db.meta_ver, db.presets_to_albums) = (2, 2, true);
        db.images = (0..20).map(|n| img(&format!("j{n}"), n)).collect();
        std::fs::write(d.join("db.json"), serde_json::to_vec(&db).unwrap()).unwrap();
        std::fs::write(d.join("db.json.bak"), b"{}").unwrap();
        let mut from_json: Db = serde_json::from_slice(&std::fs::read(d.join("db.json")).unwrap()).unwrap();
        from_json.forget_saved();
        convert_json(&mut from_json, &d).unwrap();
        assert!(d.join(FILE).exists() && d.join("db.json.v016").exists() && d.join("db.json.bak.v016").exists());
        assert!(!d.join("db.json.bak").exists() && !d.join(format!("{FILE}.tmp")).exists());
        assert!(is_stub(&d.join("db.json")));
        assert!(serde_json::from_slice::<Db>(&std::fs::read(d.join("db.json")).unwrap()).is_err(), "old exes cannot read the stub");
        let c = open(&d.join(FILE)).unwrap();
        assert_eq!(same_db(&db, &load(&c).unwrap()), None);
        assert!(from_json.take_delta().unwrap().is_empty(), "conversion counts as saved");
        drop(c);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn newer_schema_is_read_only() {
        let d = dir("schema");
        let c = open(&d.join(FILE)).unwrap();
        c.execute("INSERT INTO meta(key, value) VALUES('schema', '99')", []).unwrap();
        assert!(load(&c).unwrap().read_only);
        drop(c);
        let _ = std::fs::remove_dir_all(&d);
    }
}
