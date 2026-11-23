//! Background image loading: small thumbnails (cached on disk by picture id)
//! and large views (preview / viewer / compare, memory only).

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};

use eframe::egui;

pub const THUMB_PX: u32 = 384;
const MAGIC: &[u8; 8] = b"IMGRTH1\0";
const MAX_QUEUE: usize = 4000;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Thumb,
    /// Large view, longest side limited to this many pixels.
    Big(u32),
}

pub struct Loaded {
    pub id: String,
    pub kind: Kind,
    pub image: Option<egui::ColorImage>,
}

struct Job {
    id: String,
    path: PathBuf,
    kind: Kind,
}

struct Shared {
    thumbs: Mutex<VecDeque<Job>>,
    bigs: Mutex<VecDeque<Job>>,
    cv: Condvar,
    cache_dir: PathBuf,
    tx: Mutex<Sender<Loaded>>,
    ctx: egui::Context,
}

pub struct Loader {
    shared: Arc<Shared>,
    pub rx: Receiver<Loaded>,
}

impl Loader {
    pub fn new(ctx: egui::Context, cache_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&cache_dir);
        let (tx, rx) = channel();
        let shared = Arc::new(Shared {
            thumbs: Mutex::new(VecDeque::new()),
            bigs: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
            cache_dir,
            tx: Mutex::new(tx),
            ctx,
        });
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        for _ in 0..(n / 2).clamp(2, 6) {
            let s = shared.clone();
            std::thread::Builder::new().name("load".into()).spawn(move || worker(s)).ok();
        }
        Self { shared, rx }
    }

    pub fn request(&self, id: &str, path: PathBuf, kind: Kind) {
        let job = Job { id: id.to_string(), path, kind };
        match kind {
            Kind::Thumb => {
                let mut q = self.shared.thumbs.lock().unwrap();
                q.push_back(job);
                while q.len() > MAX_QUEUE {
                    q.pop_front();
                }
            }
            Kind::Big(_) => {
                let mut q = self.shared.bigs.lock().unwrap();
                q.push_back(job);
                while q.len() > 16 {
                    q.pop_front();
                }
            }
        }
        self.shared.cv.notify_one();
    }

    /// Forget waiting thumbnail jobs (after the search changed).
    pub fn clear_thumbs(&self) -> Vec<String> {
        self.shared.thumbs.lock().unwrap().drain(..).map(|j| j.id).collect()
    }
}

fn next_job(s: &Shared) -> Job {
    loop {
        // big views first (the user is looking at it), newest first
        if let Some(j) = s.bigs.lock().unwrap().pop_back() {
            return j;
        }
        let mut q = s.thumbs.lock().unwrap();
        if let Some(j) = q.pop_back() {
            return j;
        }
        let _q = s.cv.wait(q).unwrap();
    }
}

fn worker(s: Arc<Shared>) {
    loop {
        let job = next_job(&s);
        let image = match job.kind {
            Kind::Thumb => load_thumb(&job.path, &job.id, &s.cache_dir).ok(),
            Kind::Big(px) => load_big(&job.path, px).ok(),
        };
        let image = image.map(|img| egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()));
        let _ = s.tx.lock().unwrap().send(Loaded { id: job.id, kind: job.kind, image });
        s.ctx.request_repaint();
    }
}

fn decode(path: &Path) -> Result<image::DynamicImage, String> {
    image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())
}

pub fn load_big(path: &Path, max_px: u32) -> Result<image::RgbaImage, String> {
    let img = decode(path)?;
    let img = if img.width() > max_px || img.height() > max_px { img.resize(max_px, max_px, image::imageops::FilterType::Triangle) } else { img };
    Ok(flatten_on_dark(img.to_rgba8()))
}

fn cache_path(cache_dir: &Path, id: &str) -> PathBuf {
    let sub = id.get(..2).unwrap_or("00");
    cache_dir.join(sub).join(format!("{id}.thb"))
}

pub fn load_thumb(path: &Path, id: &str, cache_dir: &Path) -> Result<image::RgbaImage, String> {
    let cp = cache_path(cache_dir, id);
    if let Ok(data) = std::fs::read(&cp) {
        if let Some(img) = decode_cache(&data) {
            return Ok(img);
        }
    }
    let img = decode(path)?;
    let small = if img.width() > THUMB_PX || img.height() > THUMB_PX { img.thumbnail(THUMB_PX, THUMB_PX) } else { img };
    let rgba = flatten_on_dark(small.to_rgba8());
    if let Some(bytes) = encode_cache(&rgba) {
        let _ = std::fs::create_dir_all(cp.parent().unwrap());
        let tmp = cp.with_extension("tmp");
        if let Ok(mut f) = std::fs::File::create(&tmp) {
            if f.write_all(&bytes).is_ok() {
                drop(f);
                let _ = std::fs::rename(&tmp, &cp);
            }
        }
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(rgba)
}

fn flatten_on_dark(mut img: image::RgbaImage) -> image::RgbaImage {
    const BG: [u8; 3] = [32, 32, 34];
    for p in img.pixels_mut() {
        let a = p[3] as u32;
        if a < 255 {
            for c in 0..3 {
                p[c] = ((p[c] as u32 * a + BG[c] as u32 * (255 - a)) / 255) as u8;
            }
            p[3] = 255;
        }
    }
    img
}

fn encode_cache(img: &image::RgbaImage) -> Option<Vec<u8>> {
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let mut out = Vec::with_capacity(40_000);
    out.extend_from_slice(MAGIC);
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88);
    enc.encode_image(&rgb).ok()?;
    Some(out)
}

fn decode_cache(data: &[u8]) -> Option<image::RgbaImage> {
    if data.len() < 8 || &data[..8] != MAGIC {
        return None;
    }
    image::load_from_memory_with_format(&data[8..], image::ImageFormat::Jpeg).ok().map(|i| i.to_rgba8())
}

/// Remove cached thumbnails whose picture is no longer in the library.
pub fn prune(cache_dir: &Path, keep: &std::collections::HashSet<String>) -> usize {
    let mut n = 0;
    let Ok(rd) = std::fs::read_dir(cache_dir) else { return 0 };
    for sub in rd.flatten() {
        if let Ok(rd2) = std::fs::read_dir(sub.path()) {
            for f in rd2.flatten() {
                let name = f.file_name().to_string_lossy().to_string();
                if let Some(id) = name.strip_suffix(".thb") {
                    if !keep.contains(id) && std::fs::remove_file(f.path()).is_ok() {
                        n += 1;
                    }
                }
            }
        }
    }
    n
}
