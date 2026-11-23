//! File jobs that run outside the UI thread: export (for uploads) and the
//! watch-folder check for automatic import.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fmt {
    /// Plain copy: the ComfyUI prompt stays inside the file.
    Keep,
    /// Re-encode as JPEG (no prompt / metadata).
    Jpg(u8),
    /// Re-encode as PNG (no prompt / metadata).
    Png,
}

pub struct ExportReport {
    pub done: usize,
    pub failed: Vec<String>,
    pub out_dir: PathBuf,
    /// ids that were exported (for tagging them afterwards)
    pub ids: Vec<String>,
}

/// files: (picture id, source path)
pub fn export(files: &[(String, PathBuf)], dir: &Path, fmt: Fmt, upload_names: bool, progress: &dyn Fn(usize, usize)) -> ExportReport {
    let mut rep = ExportReport { done: 0, failed: Vec::new(), out_dir: dir.to_path_buf(), ids: Vec::new() };
    if let Err(e) = std::fs::create_dir_all(dir) {
        rep.failed.push(format!("{}: {e}", dir.display()));
        return rep;
    }
    let total = files.len();
    for (k, (id, src)) in files.iter().enumerate() {
        progress(k, total);
        let orig_ext = src.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
        let ext = match fmt {
            Fmt::Keep => orig_ext.as_str(),
            Fmt::Jpg(_) => "jpg",
            Fmt::Png => "png",
        };
        let stem = if upload_names {
            crate::db::upload_name(k as u64)
        } else {
            src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "image".into())
        };
        let mut dest = dir.join(format!("{stem}.{ext}"));
        let mut n = 1;
        while dest.exists() {
            dest = dir.join(format!("{stem}_{n}.{ext}"));
            n += 1;
        }
        let r: Result<(), String> = match fmt {
            Fmt::Keep => std::fs::copy(src, &dest).map(|_| ()).map_err(|e| e.to_string()),
            Fmt::Jpg(q) => image::open(src).map_err(|e| e.to_string()).and_then(|img| {
                let rgb = flatten_white(img.to_rgba8());
                let f = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
                let mut w = std::io::BufWriter::new(f);
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, q).encode_image(&rgb).map_err(|e| e.to_string())
            }),
            Fmt::Png => image::open(src)
                .map_err(|e| e.to_string())
                .and_then(|img| img.save_with_format(&dest, image::ImageFormat::Png).map_err(|e| e.to_string())),
        };
        match r {
            Ok(()) => {
                rep.done += 1;
                rep.ids.push(id.clone());
            }
            Err(e) => {
                let _ = std::fs::remove_file(&dest);
                rep.failed.push(format!("{}: {e}", src.display()));
            }
        }
    }
    progress(total, total);
    rep
}

fn flatten_white(img: image::RgbaImage) -> image::RgbImage {
    let (w, h) = img.dimensions();
    let mut out = image::RgbImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels() {
        let a = p[3] as u32;
        let c = |v: u8| ((v as u32 * a + 255 * (255 - a)) / 255) as u8;
        out.put_pixel(x, y, image::Rgb([c(p[0]), c(p[1]), c(p[2])]));
    }
    out
}

/// Seen files of watch folders: path -> (size, mtime).
pub type Seen = HashMap<String, (u64, u64)>;

pub fn load_seen() -> Seen {
    std::fs::read(crate::db::data_dir().join("watch_seen.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_seen(s: &Seen) {
    let d = crate::db::data_dir();
    let _ = std::fs::create_dir_all(&d);
    if let Ok(b) = serde_json::to_vec(s) {
        let tmp = d.join("watch_seen.json.tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(tmp, d.join("watch_seen.json"));
        }
    }
}

/// New or changed pictures in the watch folders (not touched in the last 8 s,
/// so files still being written are left for the next round).
/// New / changed pictures in the watch folders, plus every picture key still there (to prune `seen`).
pub fn watch_new(dirs: &[String], seen: &Seen) -> (Vec<(PathBuf, String, u64, u64)>, std::collections::HashSet<String>) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let src = crate::db::src_dir();
    let mut out = Vec::new();
    let mut alive = std::collections::HashSet::new();
    for d in dirs {
        let d = PathBuf::from(d.trim());
        if !d.is_dir() || crate::db::path_inside(&d, &src) {
            continue;
        }
        let mut st = vec![d];
        while let Some(dir) = st.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() {
                    if !crate::db::skip_dir(&p) {
                        st.push(p);
                    }
                    continue;
                }
                if !crate::db::is_image(&p) {
                    continue;
                }
                let Ok(md) = e.metadata() else { continue };
                let (size, mt) = (md.len(), crate::db::mtime_of(&md));
                if mt + 8 > now {
                    continue;
                }
                let key = p.to_string_lossy().to_string();
                alive.insert(key.clone());
                if seen.get(&key) != Some(&(size, mt)) {
                    out.push((p, key, size, mt));
                }
            }
        }
    }
    (out, alive)
}
