//! Search text -> filter. Words are AND-ed; a leading '-' negates; 'a|b' inside
//! one word means either.
//!
//!   #pixiv          has the hashtag            -#pixiv     does not
//!   score>=4  score:5  score<3  score:0 (no score)
//!   preset:photoB      exact preset (from g_<preset>_ file names or set by hand); preset:없음 = none
//!   ckpt:obsess     checkpoint name contains   lora:pastel
//!   seed:12345      folder:00001    name:003640
//!   is:trash  is:missing  is:scored  is:unscored  is:tagged  is:untagged
//!   sort:new  sort:old  sort:name  sort:score  sort:seed  sort:size  sort:random
//!   lib:char/ch001_alice   examples of a library entry
//!   album:3         pictures directly in album 3 (ids: see the album list); album:none = in no album
//!   anything else   found in prompt, file name, tags, preset or checkpoint
//! Pictures in the trash (or missing) are hidden unless asked for with is:trash / is:missing.

use crate::db::Img;

#[derive(Debug, Clone, PartialEq)]
enum Atom {
    Tag(String),
    Score(Cmp, u8),
    Preset(String),
    Ckpt(String),
    Lora(String),
    Seed(u64),
    Folder(String),
    Name(String),
    Is(String),
    /// examples of a library entry (lib:char/ch001_alice)
    Lib(String),
    /// album:3 (Some(3)) / album:none (None) / album:top (Some(0) = the album view's top: pictures in no album)
    Album(Option<u32>),
    /// album:3+ = album 3 and all its sub albums
    AlbumTree(u32),
    /// time window [lo, hi) in unix seconds; mtime = file time ("date:"), else import time ("added:")
    Time { mtime: bool, lo: i64, hi: i64 },
    Text(String),
}

/// Local midnight of a date, unix seconds.
fn day_start(d: chrono::NaiveDate) -> i64 {
    use chrono::TimeZone;
    chrono::Local
        .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .map(|t| t.timestamp())
        .unwrap_or(0)
}

/// added:today / yesterday / 3d (last 3 days) / 2026-11-15 / 11-15, with : >= > <= <
fn parse_time(mtime: bool, rest: &str) -> Option<Atom> {
    let (op, v) = if let Some(v) = rest.strip_prefix(">=") {
        (">=", v)
    } else if let Some(v) = rest.strip_prefix("<=") {
        ("<=", v)
    } else if let Some(v) = rest.strip_prefix('>') {
        (">", v)
    } else if let Some(v) = rest.strip_prefix('<') {
        ("<", v)
    } else {
        (":", rest.strip_prefix(':').or_else(|| rest.strip_prefix('='))?)
    };
    let today = chrono::Local::now().date_naive();
    let (lo, hi) = match v {
        "today" | "오늘" => (day_start(today), day_start(today) + 86400),
        "yesterday" | "어제" => (day_start(today) - 86400, day_start(today)),
        _ if v.ends_with('d') && v[..v.len() - 1].parse::<i64>().is_ok() => {
            let n: i64 = v[..v.len() - 1].parse().ok()?;
            (day_start(today) - (n - 1).max(0) * 86400, i64::MAX)
        }
        _ => {
            let d = chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d")
                .ok()
                .or_else(|| chrono::NaiveDate::parse_from_str(&format!("{}-{v}", today.format("%Y")), "%Y-%m-%d").ok())?;
            (day_start(d), day_start(d) + 86400)
        }
    };
    let (lo, hi) = match op {
        ">=" => (lo, i64::MAX),
        ">" => (hi, i64::MAX),
        "<" => (i64::MIN, lo),
        "<=" => (i64::MIN, hi),
        _ => (lo, hi),
    };
    Some(Atom::Time { mtime, lo, hi })
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Cmp {
    Eq,
    Ge,
    Le,
    Gt,
    Lt,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Sort {
    #[default]
    New,
    Old,
    Name,
    Score,
    Seed,
    Size,
    Random,
}

impl Sort {
    pub const ALL: [Sort; 7] = [Sort::New, Sort::Old, Sort::Name, Sort::Score, Sort::Seed, Sort::Size, Sort::Random];
    pub fn label(self) -> &'static str {
        match self {
            Sort::New => crate::i18n::tr("최근 것부터", "Newest first"),
            Sort::Old => crate::i18n::tr("오래된 것부터", "Oldest first"),
            Sort::Name => crate::i18n::tr("이름순", "By name"),
            Sort::Score => crate::i18n::tr("점수 높은 순", "Highest score"),
            Sort::Seed => crate::i18n::tr("시드순(비교용)", "By seed (for comparing)"),
            Sort::Size => crate::i18n::tr("파일 큰 순", "Largest file"),
            Sort::Random => crate::i18n::tr("무작위(채점용)", "Random (for rating)"),
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Sort::New => "new",
            Sort::Old => "old",
            Sort::Name => "name",
            Sort::Score => "score",
            Sort::Seed => "seed",
            Sort::Size => "size",
            Sort::Random => "random",
        }
    }
    pub fn from_key(s: &str) -> Option<Sort> {
        Sort::ALL.into_iter().find(|x| x.key() == s)
    }
}

#[derive(Debug, Clone)]
struct Term {
    neg: bool,
    any: Vec<Atom>,
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    terms: Vec<Term>,
    pub sort: Option<Sort>,
    show_trash: bool,
    show_missing: bool,
    /// Picture ids of library entries used by lib: words (filled in by the app).
    pub lib: std::collections::HashMap<String, std::collections::HashSet<String>>,
    /// album:3+ -> album 3 and its sub albums (filled in by the app).
    pub trees: std::collections::HashMap<u32, std::collections::HashSet<u32>>,
}

/// Album paths and names ("trip/test", "trip"; lower case) -> album ids (true =
/// a path), kept up to date by the app, so a search can name an album:
/// album:trip  album:trip/test+. A path wins over a name ("test" = the top album test).
static ALBUM_NAMES: std::sync::RwLock<Vec<(String, u32, bool)>> = std::sync::RwLock::new(Vec::new());

pub fn set_album_names(v: Vec<(String, u32, bool)>) {
    if let Ok(mut w) = ALBUM_NAMES.write() {
        *w = v;
    }
}

/// album:<v> -> atoms: a number is an album id, else an album name or path
/// (several albums may share a name); an unknown name matches nothing.
fn album_atoms(v: &str) -> Vec<Atom> {
    match v {
        "none" | "없음" | "무소속" => return vec![Atom::Album(None)],
        // the album view's top: the top albums (shown by the app) and the pictures in no album
        "top" | "맨위" => return vec![Atom::Album(Some(0))],
        _ => {}
    }
    let (base, tree) = match v.strip_suffix('+') {
        Some(b) if !b.is_empty() => (b, true),
        _ => (v, false),
    };
    let mut ids: Vec<u32> = match base.parse::<u32>() {
        Ok(n) => vec![n],
        Err(_) => ALBUM_NAMES
            .read()
            .map(|m| {
                let by = |path: bool| m.iter().filter(|x| x.2 == path && x.0 == base).map(|x| x.1).collect::<Vec<u32>>();
                let p = by(true);
                if p.is_empty() { by(false) } else { p }
            })
            .unwrap_or_default(),
    };
    ids.dedup();
    if ids.is_empty() {
        ids.push(u32::MAX);
    }
    ids.into_iter().map(|id| if tree { Atom::AlbumTree(id) } else { Atom::Album(Some(id)) }).collect()
}

/// A word for the search text: quoted when it has spaces ("album:my album+").
fn quote_word(w: &str) -> String {
    if w.chars().any(char::is_whitespace) { format!("\"{w}\"") } else { w.to_string() }
}

fn join_words(v: Vec<String>) -> String {
    v.iter().map(|w| quote_word(w)).collect::<Vec<_>>().join(" ")
}

fn split_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn parse_atom(w: &str) -> Option<Atom> {
    let lw = w.to_lowercase();
    if let Some(t) = lw.strip_prefix('#') {
        return (!t.is_empty()).then(|| Atom::Tag(t.to_string()));
    }
    if let Some(r) = lw.strip_prefix("score") {
        let (cmp, num) = if let Some(n) = r.strip_prefix(">=") {
            (Cmp::Ge, n)
        } else if let Some(n) = r.strip_prefix("<=") {
            (Cmp::Le, n)
        } else if let Some(n) = r.strip_prefix('>') {
            (Cmp::Gt, n)
        } else if let Some(n) = r.strip_prefix('<') {
            (Cmp::Lt, n)
        } else if let Some(n) = r.strip_prefix(':').or_else(|| r.strip_prefix('=')) {
            (Cmp::Eq, n)
        } else {
            return Some(Atom::Text(lw));
        };
        return num.parse().ok().map(|n| Atom::Score(cmp, n));
    }
    for (key, mt) in [("added", false), ("date", true)] {
        if let Some(r) = lw.strip_prefix(key) {
            if r.starts_with([':', '=', '<', '>']) {
                return Some(parse_time(mt, r).unwrap_or(Atom::Text(lw.clone())));
            }
        }
    }
    if let Some((k, v)) = lw.split_once(':') {
        if v.is_empty() {
            return None;
        }
        let v = v.to_string();
        return Some(match k {
            "preset" | "p" => Atom::Preset(v),
            "ckpt" | "model" => Atom::Ckpt(v),
            "lora" => Atom::Lora(v),
            "seed" => return v.parse().ok().map(Atom::Seed),
            "folder" => Atom::Folder(v),
            "name" => Atom::Name(v),
            "is" => Atom::Is(v),
            "lib" => Atom::Lib(v),
            "album" | "앨범" => return album_atoms(&v).into_iter().next(),
            _ => Atom::Text(lw),
        });
    }
    Some(Atom::Text(lw))
}

/// Like `parse_atom`, but an album name shared by several albums gives them all.
fn parse_atoms(w: &str) -> Vec<Atom> {
    let lw = w.to_lowercase();
    for k in ["album:", "앨범:"] {
        if let Some(v) = lw.strip_prefix(k) {
            return if v.is_empty() { Vec::new() } else { album_atoms(v) };
        }
    }
    parse_atom(w).into_iter().collect()
}

impl Query {
    pub fn parse(s: &str) -> Query {
        let mut q = Query::default();
        for w in split_words(s) {
            let (neg, body) = match w.strip_prefix('-') {
                Some(b) if !b.is_empty() => (true, b),
                _ => (false, w.as_str()),
            };
            if let Some(k) = body.to_lowercase().strip_prefix("sort:") {
                q.sort = Sort::from_key(k);
                continue;
            }
            let any: Vec<Atom> = body.split('|').flat_map(parse_atoms).collect();
            if any.is_empty() {
                continue;
            }
            for a in &any {
                if !neg {
                    match a {
                        Atom::Is(v) if v == "trash" => q.show_trash = true,
                        Atom::Is(v) if v == "missing" => q.show_missing = true,
                        _ => {}
                    }
                }
            }
            q.terms.push(Term { neg, any });
        }
        q
    }

    /// Albums named by album:N+ words (their sub albums count too).
    pub fn album_trees(&self) -> Vec<u32> {
        self.terms.iter().flat_map(|t| t.any.iter()).filter_map(|a| if let Atom::AlbumTree(x) = a { Some(*x) } else { None }).collect()
    }

    /// Library entry keys named by lib: words.
    pub fn lib_keys(&self) -> Vec<String> {
        let mut v = Vec::new();
        for t in &self.terms {
            for a in &t.any {
                if let Atom::Lib(k) = a {
                    v.push(k.clone());
                }
            }
        }
        v
    }

    pub fn matches(&self, i: &Img) -> bool {
        if i.trash && !self.show_trash {
            return false;
        }
        if i.missing && !self.show_missing {
            return false;
        }
        self.terms.iter().all(|t| t.any.iter().any(|a| atom(a, i, self)) != t.neg)
    }
}

fn atom(a: &Atom, i: &Img, q: &Query) -> bool {
    match a {
        Atom::Tag(t) => i.tags.iter().any(|x| x == t),
        Atom::Lib(k) => q.lib.get(k).is_some_and(|s| s.contains(&i.id)),
        Atom::Time { mtime, lo, hi } => {
            let t = if *mtime { i.mtime as i64 } else { i.added };
            t >= *lo && t < *hi
        }
        Atom::Score(c, n) => {
            let s = i.score;
            match c {
                Cmp::Eq => s == *n,
                Cmp::Ge => s >= *n,
                Cmp::Le => s <= *n,
                Cmp::Gt => s > *n,
                Cmp::Lt => s < *n,
            }
        }
        Atom::Preset(p) => {
            if p == "없음" || p == "none" {
                i.preset.is_empty()
            } else {
                i.preset.to_lowercase() == *p
            }
        }
        Atom::Album(Some(0)) | Atom::Album(None) => i.albums.is_empty(),
        Atom::Album(Some(a)) => i.albums.contains(a),
        Atom::AlbumTree(a) => q.trees.get(a).is_some_and(|t| i.albums.iter().any(|x| t.contains(x))),
        Atom::Ckpt(c) => i.ckpt.to_lowercase().contains(c.as_str()),
        Atom::Lora(l) => i.loras.iter().any(|x| x.to_lowercase().contains(l.as_str())),
        Atom::Seed(s) => i.seed == Some(*s),
        Atom::Folder(f) => i.folder().to_lowercase().starts_with(f.as_str()),
        Atom::Name(n) => i.name().to_lowercase().contains(n.as_str()),
        Atom::Is(v) => match v.as_str() {
            "trash" => i.trash,
            "missing" => i.missing,
            "scored" => i.score > 0,
            "unscored" => i.score == 0,
            "tagged" => !i.tags.is_empty(),
            "untagged" => i.tags.is_empty(),
            _ => false,
        },
        Atom::Text(t) => {
            i.prompt.to_lowercase().contains(t.as_str())
                || i.name().to_lowercase().contains(t.as_str())
                || i.tags.iter().any(|x| x.contains(t.as_str()))
                || i.preset.to_lowercase() == *t
                || i.ckpt.to_lowercase().contains(t.as_str())
        }
    }
}

/// Add or remove one word (e.g. "#pixiv") in a search text, keeping the rest.
pub fn toggle_word(q: &str, word: &str) -> String {
    let words = split_words(q);
    let lw = word.to_lowercase();
    if words.iter().any(|w| w.to_lowercase() == lw) {
        join_words(words.into_iter().filter(|w| w.to_lowercase() != lw).collect())
    } else {
        let mut v = words;
        // a word and its negation can't both be there
        let other = if let Some(x) = lw.strip_prefix('-') { x.to_string() } else { format!("-{lw}") };
        v.retain(|w| w.to_lowercase() != other);
        v.push(word.to_string());
        join_words(v)
    }
}

/// Replace every word with this prefix (e.g. "score") by `word` (or remove them when None).
pub fn set_prefixed(q: &str, prefix: &str, word: Option<&str>) -> String {
    let mut v: Vec<String> = split_words(q).into_iter().filter(|w| !w.to_lowercase().starts_with(prefix)).collect();
    if let Some(w) = word {
        v.push(w.to_string());
    }
    join_words(v)
}

/// The album a search shows: `Some(Some(id))` for album:id, `Some(None)` for
/// album:none, `None` when the search names no (single, non-negated) album.
pub fn album_word(q: &str) -> Option<Option<u32>> {
    for w in split_words(q) {
        if let Some(Atom::Album(a)) = parse_atom(&w) {
            return Some(a);
        }
    }
    None
}

/// The search text for one album (keeps nothing else: an album is a place).
/// `name`: how the search names the album (its name, path or id; see `App::album_ref`).
pub fn album_query(a: Option<u32>, name: &str) -> String {
    match a {
        Some(0) => "album:top".to_string(),
        Some(_) => quote_word(&format!("album:{name}")),
        None => "album:none".to_string(),
    }
}

/// The search with every album word that names one album by its id or old
/// name rewritten by `name_of` (e.g. after an album was renamed).
pub fn rename_album_words(q: &str, name_of: impl Fn(u32) -> String) -> String {
    let words: Vec<String> = split_words(q)
        .into_iter()
        .map(|w| {
            let (neg, body) = match w.strip_prefix('-') {
                Some(b) => ("-", b.to_string()),
                None => ("", w.clone()),
            };
            let parts: Vec<String> = body
                .split('|')
                .map(|p| {
                    let lp = p.to_lowercase();
                    let Some((k, v)) = lp.split_once(':') else { return p.to_string() };
                    if !matches!(k, "album" | "앨범") || v.is_empty() {
                        return p.to_string();
                    }
                    match album_atoms(v).as_slice() {
                        [Atom::Album(Some(id))] if *id != 0 && *id != u32::MAX => format!("album:{}", name_of(*id)),
                        [Atom::AlbumTree(id)] if *id != u32::MAX => format!("album:{}+", name_of(*id)),
                        _ => p.to_string(),
                    }
                })
                .collect();
            format!("{neg}{}", parts.join("|"))
        })
        .collect();
    join_words(words)
}

/// The search without its album words (the other filters stay).
pub fn without_album(q: &str) -> String {
    split_words(q)
        .into_iter()
        .filter(|w| {
            let l = w.trim_start_matches('-').to_lowercase();
            !(l.starts_with("album:") || l.starts_with("앨범:"))
        })
        .map(|w| quote_word(&w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Does the search name an album at all (also negated or unknown ones)?
pub fn names_album(q: &str) -> bool {
    split_words(q).iter().any(|w| {
        let l = w.trim_start_matches('-').to_lowercase();
        l.starts_with("album:") || l.starts_with("앨범:")
    })
}

/// Values of the (non-negated) `key:a|key:b` word, e.g. the presets picked in the side panel.
pub fn group_values(q: &str, key: &str) -> Vec<String> {
    group_values_cased(q, key).into_iter().map(|v| v.to_lowercase()).collect()
}

/// `group_values` keeping the letters' case (album names).
pub fn group_values_cased(q: &str, key: &str) -> Vec<String> {
    let pre = format!("{key}:");
    for w in split_words(q) {
        if w.to_lowercase().starts_with(&pre) {
            return w.split('|').filter(|p| p.to_lowercase().starts_with(&pre)).map(|p| p[pre.len()..].to_string()).collect();
        }
    }
    Vec::new()
}

/// Replace the `key:` word by a group of values (a|b ...), or remove it when empty.
pub fn set_group(q: &str, key: &str, vals: &[String]) -> String {
    let pre = format!("{key}:");
    if vals.is_empty() {
        return set_prefixed(q, &pre, None);
    }
    let w = vals.iter().map(|v| format!("{pre}{v}")).collect::<Vec<_>>().join("|");
    set_prefixed(q, &pre, Some(&w))
}

pub fn has_word(q: &str, word: &str) -> bool {
    let lw = word.to_lowercase();
    split_words(q).iter().any(|w| w.to_lowercase() == lw)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn img() -> Img {
        Img {
            path: "00001/g_animeA_003640.png".into(),
            score: 4,
            tags: vec!["pixiv".into(), "alice".into()],
            preset: "animeA".into(),
            ckpt: "OneObsession24".into(),
            loras: vec!["style_pastel1".into()],
            prompt: "1girl, white hair, bikini".into(),
            seed: Some(42),
            ..Default::default()
        }
    }
    #[test]
    fn basics() {
        let i = img();
        for (q, want) in [
            ("", true),
            ("#pixiv", true),
            ("-#pixiv", false),
            ("#nope|#alice", true),
            ("score>=4", true),
            ("score>4", false),
            ("score:4 preset:animea", true),
            ("preset:photoB", false),
            ("preset:photoB|preset:animea", true),
            ("preset:없음", false),
            ("ckpt:obsess lora:pastel", true),
            ("seed:42", true),
            ("seed:41", false),
            ("folder:00001 name:3640", true),
            ("bikini", true),
            ("\"white hair\"", true),
            ("is:unscored", false),
            ("is:tagged", true),
            ("sort:score bikini", true),
        ] {
            assert_eq!(Query::parse(q).matches(&i), want, "{q}");
        }
        assert_eq!(Query::parse("sort:score").sort, Some(Sort::Score));
    }
    #[test]
    fn trash_hidden_unless_asked() {
        let mut i = img();
        i.trash = true;
        assert!(!Query::parse("").matches(&i));
        assert!(Query::parse("is:trash").matches(&i));
        assert!(!Query::parse("-is:trash").matches(&i));
    }
    #[test]
    fn word_editing() {
        assert_eq!(toggle_word("a #x", "#x"), "a");
        assert_eq!(toggle_word("a", "#x"), "a #x");
        assert_eq!(toggle_word("a -#x", "#x"), "a #x");
        assert_eq!(set_prefixed("a score>=3", "score", Some("score:5")), "a score:5");
        assert_eq!(set_prefixed("a score>=3", "score", None), "a");
    }

    #[test]
    fn album_words() {
        let a = Img { id: "a".into(), albums: vec![3, 7], ..Default::default() };
        let n = Img { id: "n".into(), ..Default::default() };
        let q = Query::parse("album:3");
        assert!(q.matches(&a) && !q.matches(&n));
        let q = Query::parse("album:none");
        assert!(!q.matches(&a) && q.matches(&n));
        assert!(Query::parse("앨범:없음").matches(&n));
        assert!(!Query::parse("-album:7").matches(&a));
        assert_eq!(album_word("x album:7"), Some(Some(7)));
        assert_eq!(album_word("album:none"), Some(None));
        assert_eq!(album_word("#pixiv"), None);
        assert_eq!(album_query(Some(4), "4"), "album:4");
        assert_eq!(album_query(Some(4), "my album"), "\"album:my album\"");
        assert_eq!(album_word("album:top"), Some(Some(0)));
        assert_eq!(album_query(Some(0), "x"), "album:top");
        assert!(!Query::parse("album:top").matches(&a));
        assert_eq!(without_album("ckpt:x album:3 #a"), "ckpt:x #a");
        assert!(names_album("-album:3") && !names_album("ckpt:x"));
    }

    #[test]
    fn album_tree_word() {
        let mut i = img();
        i.albums = vec![5];
        let mut q = Query::parse("album:3+");
        assert_eq!(q.album_trees(), vec![3]);
        assert!(!q.matches(&i));
        q.trees.insert(3, [3, 5].into_iter().collect());
        assert!(q.matches(&i));
        assert!(!Query::parse("album:3").matches(&i));
        assert!(!Query::parse("album:top").matches(&i));
        i.albums.clear();
        assert!(Query::parse("album:top").matches(&i));
    }

    #[test]
    fn album_names() {
        set_album_names(vec![
            ("qztrip".into(), 41, true),
            ("qztrip".into(), 41, false),
            ("qz test".into(), 42, false),
            ("qztrip/qz test".into(), 42, true),
            ("qz2/qz test".into(), 43, true),
            ("qz test".into(), 43, false),
        ]);
        let mut i = img();
        i.albums = vec![42];
        assert!(Query::parse("album:qztrip+").album_trees() == vec![41]);
        assert!(Query::parse("album:QZTRIP/\"qz test\"").matches(&i));
        assert!(Query::parse("\"album:qz test\"").matches(&i));
        i.albums = vec![43];
        assert!(Query::parse("\"album:qz test\"").matches(&i) && !Query::parse("\"album:qztrip/qz test\"").matches(&i));
        assert!(!Query::parse("album:nosuchalbum").matches(&i));
        assert_eq!(album_word("album:qztrip"), Some(Some(41)));
        assert_eq!(toggle_word("\"album:qz test\"", "#a"), "\"album:qz test\" #a");
        assert_eq!(set_group("x", "album", &["qz test+".to_string(), "qztrip+".to_string()]), "x \"album:qz test+|album:qztrip+\"");
        assert_eq!(rename_album_words("x album:41+|album:none -album:qztrip", |id| format!("n{id}")), "x album:n41+|album:none -album:n41");
        assert_eq!(group_values_cased("\"album:Qz test+|album:none\"", "album"), vec!["Qz test+", "none"]);
    }
}
