//! Library ("사전"): characters, outfits, expressions, poses and presets that
//! live as files in ComfyUI's _gen folder, with example pictures from imgs.
//!
//! * txt entries (char / outfit / face / pose): the file holds comma separated
//!   tags. Example pictures are found automatically: pictures whose prompt has
//!   (almost) all of the entry's tags. The user can pin / hide examples in imgr.
//! * presets: _gen/presets/<name>.json (and presets/testing). Examples are the
//!   pictures made with that preset (file name g_<preset>_NNNNNN).
//!
//! imgr never changes these files on its own; it writes one only when the user
//! presses 저장 / 새 항목 in the library screen (old text goes to data/lib_backup).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::db::Img;
use crate::i18n::tr;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Char,
    Outfit,
    Face,
    Pose,
    Preset,
    TestPreset,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Char, Kind::Outfit, Kind::Face, Kind::Pose, Kind::Preset, Kind::TestPreset];
    pub fn label(self) -> &'static str {
        match self {
            Kind::Char => tr("캐릭터", "Characters"),
            Kind::Outfit => tr("의상", "Outfits"),
            Kind::Face => tr("표정", "Expressions"),
            Kind::Pose => tr("포즈·구도", "Poses / framing"),
            Kind::Preset => tr("프리셋", "Presets"),
            Kind::TestPreset => tr("시험 프리셋", "Test presets"),
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Kind::Char => "char",
            Kind::Outfit => "outfit",
            Kind::Face => "face",
            Kind::Pose => "pose",
            Kind::Preset => "preset",
            Kind::TestPreset => "testpreset",
        }
    }
    pub fn from_key(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key() == s)
    }
    /// Folder inside _gen.
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Char => "topics\\char",
            Kind::Outfit => "topics\\outfit",
            Kind::Face => "topics\\face",
            Kind::Pose => "topics\\pose",
            Kind::Preset => "presets",
            Kind::TestPreset => "presets\\testing",
        }
    }
    pub fn is_preset(self) -> bool {
        matches!(self, Kind::Preset | Kind::TestPreset)
    }
    /// Suggested file name start for a new entry.
    pub fn prefix(self) -> &'static str {
        match self {
            Kind::Char => "ch",
            Kind::Outfit => "of",
            Kind::Face => "fc",
            Kind::Pose => "ps",
            _ => "",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: Kind,
    pub name: String,
    /// "char/ch001_alice" (lower case, used in lib: searches and LibMeta)
    pub key: String,
    pub file: PathBuf,
    /// File text (txt) or a short summary (preset).
    pub text: String,
    /// Normalised tags used to find examples (txt entries).
    pub tags: Vec<String>,
    /// Preset: checkpoint / LoRAs / notes, one per line.
    pub summary: Vec<(String, String)>,
}

pub struct Folder {
    pub kind: Kind,
    pub path: PathBuf,
    pub exists: bool,
}

/// No default: the 사전 asks for the _gen folder ("폴더…") until one is set.
pub fn default_gen_dir() -> String {
    String::new()
}

/// Tag text -> comparable form: lower case, no weights "(x:1.2)", '_' = ' '.
pub fn norm_tag(s: &str) -> String {
    let mut t = s.trim().to_lowercase().replace("\\(", "\u{1}").replace("\\)", "\u{2}");
    t = t.trim_matches(|c| c == '(' || c == ')' || c == '[' || c == ']' || c == '{' || c == '}').to_string();
    if let Some((a, b)) = t.rsplit_once(':') {
        if b.trim().parse::<f32>().is_ok() {
            t = a.to_string();
        }
    }
    t = t.trim_matches(|c| c == '(' || c == ')' || c == '[' || c == ']').to_string();
    let t = t.replace('\u{1}', "(").replace('\u{2}', ")").replace('_', " ");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn split_tags(text: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with("//") || l.starts_with(';') {
            continue;
        }
        for t in l.split(',') {
            let n = norm_tag(t);
            if !n.is_empty() && !v.contains(&n) {
                v.push(n);
            }
        }
    }
    v
}

pub fn prompt_tags(prompt: &str) -> HashSet<String> {
    prompt.split([',', '\n', '|']).map(norm_tag).filter(|t| !t.is_empty()).collect()
}

/// Does a picture whose prompt has these tags show the entry?
/// Short entries need every tag; longer ones 85 %.
pub fn tags_match(entry: &[String], ptags: &HashSet<String>) -> bool {
    if entry.is_empty() {
        return false;
    }
    let have = entry.iter().filter(|t| ptags.contains(*t)).count();
    let need = if entry.len() <= 3 { entry.len() } else { (entry.len() as f32 * 0.85).ceil() as usize };
    have >= need
}

pub fn load(gen_dir: &Path) -> (Vec<Entry>, Vec<Folder>) {
    let mut entries = Vec::new();
    let mut folders = Vec::new();
    // no folder set yet: nothing (never the current folder)
    if gen_dir.as_os_str().is_empty() {
        return (entries, folders);
    }
    for kind in Kind::ALL {
        let dir = gen_dir.join(kind.folder());
        let exists = dir.is_dir();
        folders.push(Folder { kind, path: dir.clone(), exists });
        if !exists {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for f in files {
            let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if kind.is_preset() {
                if ext != "json" {
                    continue;
                }
                let text = crate::winshell::read_text(&f).unwrap_or_default();
                entries.push(Entry {
                    kind,
                    key: format!("{}/{}", kind.key(), name.to_lowercase()),
                    name,
                    file: f.clone(),
                    summary: preset_summary(&text),
                    text,
                    tags: Vec::new(),
                });
            } else {
                if ext != "txt" {
                    continue;
                }
                let text = crate::winshell::read_text(&f).unwrap_or_default();
                entries.push(Entry {
                    kind,
                    key: format!("{}/{}", kind.key(), name.to_lowercase()),
                    name,
                    file: f.clone(),
                    tags: split_tags(&text),
                    text,
                    summary: Vec::new(),
                });
            }
        }
    }
    (entries, folders)
}

fn lora_list(v: &serde_json::Value) -> String {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let arr = x.as_array()?;
                    let n = arr.first()?.as_str()?.trim_end_matches(".safetensors");
                    let w = arr.get(1).and_then(|w| w.as_f64()).map(|w| format!(" {w}")).unwrap_or_default();
                    Some(format!("{n}{w}"))
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// The parts of a preset json that tell presets apart.
pub fn preset_summary(text: &str) -> Vec<(String, String)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return vec![("오류".into(), tr("json을 읽을 수 없음", "cannot read json").into())];
    };
    let mut out = Vec::new();
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
    if let Some(n) = s("_note") {
        out.push(("메모".into(), n));
    }
    if let Some(c) = s("ckpt") {
        out.push(("체크포인트".into(), c));
    }
    let l = lora_list(v.get("extra").unwrap_or(&serde_json::Value::Null));
    if !l.is_empty() {
        out.push(("LoRA".into(), l));
    }
    if let Some(f) = v.get("faceck") {
        let mut t = String::new();
        if let Some(c) = f.get("ckpt").and_then(|x| x.as_str()) {
            t.push_str(c);
        }
        if let Some(d) = f.get("dn").and_then(|x| x.as_f64()) {
            t.push_str(&format!("  dn {d}"));
        }
        let l = lora_list(f.get("loras").unwrap_or(&serde_json::Value::Null));
        if !l.is_empty() {
            t.push_str(&format!("  ·  {l}"));
        }
        out.push(("얼굴 다시그리기".into(), t));
    }
    if let Some(h) = v.get("hires").or_else(|| v.get("hr")) {
        out.push(("hires".into(), compact(h)));
    }
    if let Some(e) = v.get("eyeck") {
        out.push(("눈".into(), compact(e)));
    }
    if let Some(h) = v.get("hand") {
        out.push(("손".into(), compact(h)));
    }
    if let Some(p) = s("pos") {
        out.push(("추가 태그".into(), p));
    }
    out
}

fn compact(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 160 { format!("{}…", s.chars().take(160).collect::<String>()) } else { s }
}

/// Lines of presets/readme.txt that talk about this preset: each line naming it,
/// plus the more-indented lines right under it.
pub fn readme_lines(gen_dir: &Path, name: &str) -> Vec<String> {
    let Some(t) = crate::winshell::read_text(&gen_dir.join("presets").join("readme.txt")) else { return Vec::new() };
    let n = name.to_lowercase();
    let indent = |l: &str| l.chars().take_while(|c| *c == '\t' || *c == ' ').map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>();
    let lines: Vec<&str> = t.lines().collect();
    let mut out: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let low = l.to_lowercase();
        if !low.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w == n) {
            continue;
        }
        let base = indent(l);
        out.push(l.trim().to_string());
        for m in lines.iter().skip(i + 1) {
            if m.trim().is_empty() || indent(m) <= base {
                break;
            }
            out.push(format!("   {}", m.trim()));
        }
    }
    out.dedup();
    out
}

/// Example pictures (ids) per entry key, best first.
pub fn find_examples(entries: &[Entry], images: &[Img], metas: &HashMap<String, crate::db::LibMeta>) -> HashMap<String, std::sync::Arc<Vec<String>>> {
    let live: Vec<&Img> = images.iter().filter(|i| !i.missing && !i.trash).collect();
    let present: HashSet<&str> = images.iter().filter(|i| !i.missing).map(|i| i.id.as_str()).collect();
    let ptags: Vec<HashSet<String>> = live.iter().map(|i| prompt_tags(&i.prompt)).collect();
    let mut out = HashMap::new();
    for e in entries {
        let meta = metas.get(&e.key);
        let hidden: HashSet<&str> = meta.map(|m| m.hidden.iter().map(|s| s.as_str()).collect()).unwrap_or_default();
        let mut found: Vec<&Img> = if e.kind.is_preset() {
            live.iter().copied().filter(|i| i.preset.eq_ignore_ascii_case(&e.name)).collect()
        } else {
            live.iter().zip(&ptags).filter(|(_, t)| tags_match(&e.tags, t)).map(|(i, _)| *i).collect()
        };
        found.retain(|i| !hidden.contains(i.id.as_str()));
        found.sort_by(|a, b| b.score.cmp(&a.score).then(b.added.cmp(&a.added)).then(b.mtime.cmp(&a.mtime)));
        let mut ids: Vec<String> = Vec::with_capacity(found.len() + 4);
        let mut seen: HashSet<&str> = HashSet::new();
        if let Some(m) = meta {
            for p in m.cover.iter().chain(m.pins.iter()) {
                if present.contains(p.as_str()) && seen.insert(p.as_str()) {
                    ids.push(p.clone());
                }
            }
        }
        for i in found {
            if seen.insert(i.id.as_str()) {
                ids.push(i.id.clone());
            }
        }
        out.insert(e.key.clone(), std::sync::Arc::new(ids));
    }
    out
}

/// Next free "ch002_" style prefix for a new entry of this kind.
pub fn next_name(entries: &[Entry], kind: Kind) -> String {
    let p = kind.prefix();
    if p.is_empty() {
        return String::new();
    }
    let max = entries
        .iter()
        .filter(|e| e.kind == kind)
        .filter_map(|e| e.name.strip_prefix(p).and_then(|r| r.get(..3)).and_then(|d| d.parse::<u32>().ok()))
        .max()
        .unwrap_or(0);
    format!("{p}{:03}_", max + 1)
}

/// Write a txt entry (new or edited). The old text goes to data/lib_backup.
pub fn write_entry(file: &Path, text: &str) -> Result<(), String> {
    if file.exists() {
        let bdir = crate::db::data_dir().join("lib_backup");
        let _ = std::fs::create_dir_all(&bdir);
        let stem = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let b = bdir.join(format!("{stem}_{}.txt", chrono::Local::now().format("%Y%m%d_%H%M%S%.3f")));
        std::fs::copy(file, b).map_err(|e| crate::trf!("백업 실패: {e}", "Backup failed: {e}"))?;
    }
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = file.with_extension("txt.tmp");
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())
}

/// Tags from a picture prompt that are worth starting a new entry with
/// (drops quality / score words that every prompt has).
pub fn tags_from_prompt(prompt: &str) -> String {
    const SKIP: &[&str] = &[
        "masterpiece", "best quality", "amazing quality", "high quality", "very aesthetic", "absurdres", "highres", "newest", "detailed background", "1girl", "solo",
        "score 9", "score 8 up", "score 7 up", "safe", "sensitive", "nsfw", "explicit", "general",
    ];
    let mut seen = HashSet::new();
    let mut v = Vec::new();
    for t in prompt.split([',', '\n']) {
        let n = norm_tag(t);
        if n.is_empty() || SKIP.contains(&n.as_str()) || n.starts_with("score ") || n.starts_with("lora") || n.starts_with('<') {
            continue;
        }
        if seen.insert(n.clone()) {
            v.push(n);
        }
    }
    v.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn norm() {
        assert_eq!(norm_tag(" (Red_Eyes:1.2) "), "red eyes");
        assert_eq!(norm_tag("large breasts"), "large breasts");
        assert_eq!(norm_tag("\\(hello\\)"), "(hello)");
        let p = prompt_tags("masterpiece, white hair, long_hair, (twintails:1.1), red eyes");
        let e = split_tags("white hair, long hair, twintails, red eyes");
        assert!(tags_match(&e, &p));
        let e2 = split_tags("white hair, long hair, twintails, red eyes, ahoge, tareme, hair flower");
        assert!(!tags_match(&e2, &p));
    }
    #[test]
    fn readme() {
        let s = preset_summary(r#"{"_note":"n","ckpt":"oneob24","extra":[["a.safetensors",0.8,"x"]]}"#);
        assert_eq!(s[0].1, "n");
        assert_eq!(s[2].1, "a 0.8");
    }
}
