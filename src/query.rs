//! Search text -> filter. Conditions are separated by commas (also by spaces around
//! command words like #tag or album:x) and AND-ed; a leading '-' negates; 'a|b' means either.
//!   long hair, scar     prompt tags, each matched as a whole tag ('_' = ' ', no case, weights ignored)
//!   __scar__  __scar  scar__   tag contains / ends with / starts with "scar"
//!
//!   #pixiv          has the hashtag            -#pixiv     does not
//!   score>=4  score:5  score<3  score:0 (no score)
//!   preset:photoB      exact preset (from g_<preset>_ file names or set by hand); preset:없음 = none
//!   ckpt:obsess     checkpoint name contains   lora:pastel
//!   seed:12345      folder:00001    name:003640
//!   is:trash  is:missing  is:scored  is:unscored  is:tagged  is:untagged
//!   sort:new  sort:old  sort:name  sort:score  sort:seed  sort:size  sort:random
//!   album:3         pictures directly in album 3 (ids: see the album list); album:none = in no album
//!   anything else   a prompt tag (or hashtag / preset with that exact name)
//! Pictures in the trash (or missing) are hidden unless asked for with is:trash / is:missing.

use crate::db::Img;

#[derive(Debug, Clone, PartialEq)]
enum Atom {
    Tag(String),
    /// a hashtag still being typed: tags starting with it
    TagStart(String),
    Score(Cmp, u8),
    Preset(String),
    Ckpt(String),
    Lora(String),
    Seed(u64),
    Folder(String),
    Name(String),
    Is(String),
    /// album:3 (Some(3)) / album:none (None) / album:top (Some(0) = the album view's top: pictures in no album)
    Album(Option<u32>),
    /// album:3+ = album 3 and all its sub albums
    AlbumTree(u32),
    /// time window [lo, hi) in unix seconds; mtime = file time ("date:"), else import time ("added:")
    Time { mtime: bool, lo: i64, hi: i64 },
    /// a prompt tag: `pat` normalised (norm_tag); any_before / any_after = `__` wildcards
    Text { pat: String, any_before: bool, any_after: bool },
}

/// Prompt tag as compared: lower case, '_' = ' ', brackets / backslashes and a
/// trailing weight (":1.2") dropped, spaces collapsed.
fn norm_tag_into(out: &mut String, raw: &str) {
    out.clear();
    let mut space = false;
    for c in raw.chars() {
        let c = if c == '_' { ' ' } else { c };
        if matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '\\') {
            continue;
        }
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.extend(c.to_lowercase());
    }
    if let Some(k) = out.rfind(':') {
        if k > 0 && out[k + 1..].trim().parse::<f32>().is_ok() {
            out.truncate(k);
            let t = out.trim_end().len();
            out.truncate(t);
        }
    }
}

/// The tags of a prompt: split at commas and line breaks, and at a full stop that ends a
/// sentence (followed by a space or the end), so "solo. A girl. jojo pose" gives "solo",
/// "A girl", "jojo pose". A full stop inside a number (":1.2") is kept.
pub fn prompt_parts(p: &str) -> impl Iterator<Item = &str> {
    let b = p.as_bytes();
    let mut start = 0;
    let mut i = 0;
    std::iter::from_fn(move || {
        while i <= b.len() {
            let end = i == b.len()
                || matches!(b[i], b',' | b'\n')
                || (b[i] == b'.' && (i + 1 == b.len() || b[i + 1].is_ascii_whitespace()));
            i += 1;
            if end {
                let part = &p[start..i - 1];
                start = i;
                return Some(part);
            }
        }
        None
    })
}

pub fn norm_tag(raw: &str) -> String {
    let mut s = String::new();
    norm_tag_into(&mut s, raw);
    s
}

/// `scar` = the whole tag; `__scar` / `scar__` / `__scar__` = ends with / starts with / contains.
fn text_atom(w: &str) -> Atom {
    let mut s = w.trim();
    let any_before = s.len() > 2 && s.starts_with("__");
    if any_before {
        s = &s[2..];
    }
    let any_after = s.len() > 2 && s.ends_with("__");
    if any_after {
        s = &s[..s.len() - 2];
    }
    Atom::Text { pat: norm_tag(s), any_before, any_after }
}

fn tag_hit(tag: &str, pat: &str, any_before: bool, any_after: bool) -> bool {
    match (any_before, any_after) {
        (false, false) => tag == pat,
        (true, false) => tag.ends_with(pat),
        (false, true) => tag.starts_with(pat),
        (true, true) => tag.contains(pat),
    }
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
/// Keys of command words; a word starting with one of them (then : = < >) or with '#'
/// stands alone, everything else is (part of) a prompt tag.
const KEYS: &[&str] = &["score", "preset", "p", "ckpt", "model", "lora", "seed", "folder", "name", "is", "album", "앨범", "sort", "added", "date"];

fn is_keyword(w: &str) -> bool {
    let l = w.trim_start_matches('-').to_lowercase();
    l.starts_with('#') || KEYS.iter().any(|k| l.strip_prefix(k).is_some_and(|r| r.starts_with([':', '=', '<', '>'])))
}

fn quote_word(w: &str) -> String {
    if w.contains(',') || (w.chars().any(char::is_whitespace) && is_keyword(w)) { format!("\"{w}\"") } else { w.to_string() }
}

/// Words written back by the program are separated by ", ".
fn join_words(v: Vec<String>) -> String {
    v.iter().map(|w| quote_word(w)).collect::<Vec<_>>().join(", ")
}

/// Search text -> its conditions. Commas separate; command words (#tag, album:x, score>=4,
/// "quoted") also stand alone between spaces; other words in a row form one prompt tag
/// ("long hair"); a '-' word starts a new one. A lone or edge '|' joins two conditions (either).
fn split_words(s: &str) -> Vec<String> {
    // tokens: (text, comma before it, was quoted)
    let mut toks: Vec<(String, bool, bool)> = Vec::new();
    let (mut cur, mut quoted, mut cur_q, mut comma) = (String::new(), false, false, false);
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur_q = true;
            }
            c if !quoted && (c.is_whitespace() || c == ',') => {
                if !cur.is_empty() {
                    toks.push((std::mem::take(&mut cur), comma, cur_q));
                    comma = false;
                }
                cur_q = false;
                if c == ',' {
                    comma = true;
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        toks.push((cur, comma, cur_q));
    }
    let mut out: Vec<String> = Vec::new();
    let mut run: Option<String> = None;
    let mut glue = false;
    fn put(out: &mut Vec<String>, w: String, glue: &mut bool) {
        if w.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if *glue => {
                last.push('|');
                last.push_str(&w);
            }
            _ => out.push(w),
        }
        *glue = false;
    }
    for (t, comma_before, q) in toks {
        if comma_before {
            if let Some(r) = run.take() {
                put(&mut out, r, &mut glue);
            }
        }
        if !q && t.chars().all(|c| c == '|') {
            if let Some(r) = run.take() {
                put(&mut out, r, &mut glue);
            }
            glue = !out.is_empty();
            continue;
        }
        let mut t = t.as_str();
        if !q && t.starts_with('|') {
            if let Some(r) = run.take() {
                put(&mut out, r, &mut glue);
            }
            glue = !out.is_empty();
            t = t.trim_start_matches('|');
        }
        let trail = !q && t.ends_with('|');
        let t = if trail { t.trim_end_matches('|') } else { t };
        if q || is_keyword(t) || t.starts_with('-') {
            if let Some(r) = run.take() {
                put(&mut out, r, &mut glue);
            }
            if q || is_keyword(t) {
                put(&mut out, t.to_string(), &mut glue);
            } else {
                run = Some(t.to_string());
            }
        } else {
            match run.as_mut() {
                Some(r) => {
                    r.push(' ');
                    r.push_str(t);
                }
                None => run = Some(t.to_string()),
            }
        }
        if trail {
            if let Some(r) = run.take() {
                put(&mut out, r, &mut glue);
            }
            glue = !out.is_empty();
        }
    }
    if let Some(r) = run.take() {
        put(&mut out, r, &mut glue);
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
            return Some(text_atom(&lw));
        };
        return num.parse().ok().map(|n| Atom::Score(cmp, n));
    }
    for (key, mt) in [("added", false), ("date", true)] {
        if let Some(r) = lw.strip_prefix(key) {
            if r.starts_with([':', '=', '<', '>']) {
                return Some(parse_time(mt, r).unwrap_or(text_atom(&lw)));
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
            "album" | "앨범" => return album_atoms(&v).into_iter().next(),
            _ => text_atom(&lw),
        });
    }
    Some(text_atom(&lw))
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

/// A condition that OR mode joins: a positive prompt tag or hashtag (filters such as
/// score:, album:, ckpt: and '-' conditions always narrow).
fn tag_like(neg: bool, any: &[Atom]) -> bool {
    !neg && !any.is_empty() && any.iter().all(|a| matches!(a, Atom::Text { .. } | Atom::Tag(_) | Atom::TagStart(_)))
}

/// The search as written to be kept (saved filter, recent searches): in OR mode the
/// comma-separated tags become one explicit a|b condition, so it means the same in AND mode.
pub fn explicit(s: &str, or: bool) -> String {
    if !or {
        return s.trim().to_string();
    }
    let words = split_words(s);
    let is_tag = |w: &String| {
        let neg = w.starts_with('-') && w.len() > 1;
        let any: Vec<Atom> = w.split('|').flat_map(parse_atoms).collect();
        tag_like(neg, &any)
    };
    let tags: Vec<String> = words.iter().filter(|w| is_tag(w)).cloned().collect();
    if tags.len() < 2 {
        return s.trim().to_string();
    }
    let mut out = vec![tags.join("|")];
    out.extend(words.into_iter().filter(|w| !is_tag(w)));
    join_words(out)
}

impl Query {
    pub fn parse(s: &str) -> Query {
        Query::parse_with(s, false, false)
    }

    /// `or`: the [AND|OR] switch is on OR, so commas between prompt tags / hashtags mean
    /// "any of them". `live`: the search box is being typed in, so its last word is not
    /// finished yet: a prompt tag there matches tags starting with it ("so" -> solo),
    /// and an unfinished '-' condition is left out until it is done.
    pub fn parse_with(s: &str, or: bool, live: bool) -> Query {
        let mut q = Query::default();
        let words = split_words(s);
        let open = live && !s.trim_end().ends_with([',', '|']);
        let last = words.len();
        let mut joined: Option<usize> = None;
        for (k, w) in words.into_iter().enumerate() {
            let typing = open && k + 1 == last;
            // a '-' on its own: a condition still to be written
            if w.chars().all(|c| c == '-') {
                continue;
            }
            let (neg, body) = match w.strip_prefix('-') {
                Some(b) if !b.is_empty() => (true, b),
                _ => (false, w.as_str()),
            };
            if typing && neg {
                continue;
            }
            if let Some(k) = body.to_lowercase().strip_prefix("sort:") {
                q.sort = Sort::from_key(k);
                continue;
            }
            let mut any: Vec<Atom> = body.split('|').flat_map(parse_atoms).collect();
            if any.is_empty() {
                continue;
            }
            if typing {
                match any.last_mut() {
                    Some(Atom::Text { any_after, .. }) => *any_after = true,
                    Some(a @ Atom::Tag(_)) => {
                        if let Atom::Tag(t) = a {
                            *a = Atom::TagStart(std::mem::take(t));
                        }
                    }
                    _ => {}
                }
            }
            if or && tag_like(neg, &any) {
                if let Some(j) = joined {
                    q.terms[j].any.extend(any);
                    continue;
                }
                joined = Some(q.terms.len());
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
        Atom::TagStart(t) => i.tags.iter().any(|x| x.starts_with(t.as_str())),
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
        // album 0 = put at the top by hand (the picture also sits in other albums)
        Atom::Album(Some(0)) => i.albums.is_empty() || i.albums.contains(&0),
        Atom::Album(None) => i.albums.iter().all(|&a| a == 0),
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
        Atom::Text { pat, any_before, any_after } => {
            if pat.is_empty() {
                return false;
            }
            let mut buf = String::new();
            let mut hit = |raw: &str| {
                norm_tag_into(&mut buf, raw);
                tag_hit(&buf, pat, *any_before, *any_after)
            };
            prompt_parts(&i.prompt).any(&mut hit) || i.tags.iter().any(|t| hit(t)) || hit(&i.preset)
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

/// Is one of `items` (e.g. "#a", "ckpt:x") a condition of the search, alone or inside an a|b group?
pub fn has_item(q: &str, items: &[&str]) -> bool {
    split_words(q).iter().any(|w| !w.starts_with('-') && w.split('|').any(|p| items.iter().any(|i| p.eq_ignore_ascii_case(i) || p.to_lowercase() == i.to_lowercase())))
}

/// The search without `items` (taken out of a|b groups too; a group left empty goes).
pub fn remove_items(q: &str, items: &[&str]) -> String {
    let same = |p: &str| items.iter().any(|i| p.eq_ignore_ascii_case(i) || p.to_lowercase() == i.to_lowercase());
    let v: Vec<String> = split_words(q)
        .into_iter()
        .filter_map(|w| {
            if w.starts_with('-') {
                return Some(w);
            }
            let parts: Vec<&str> = w.split('|').filter(|p| !same(p)).collect();
            (!parts.is_empty()).then(|| parts.join("|"))
        })
        .collect();
    join_words(v)
}

/// A click on an item in 검색 뷰: take it out when it is there; else add it - `or`: to the
/// last condition with '|' (either), otherwise as a condition of its own (all must match).
/// `same` = other spellings that count as this item (album:trip for album:trip+).
pub fn click_item(q: &str, item: &str, same: &[&str], or: bool) -> String {
    let mut all = vec![item];
    all.extend_from_slice(same);
    if has_item(q, &all) {
        return remove_items(q, &all);
    }
    let neg = format!("-{item}");
    let mut v: Vec<String> = split_words(q).into_iter().filter(|w| !w.eq_ignore_ascii_case(&neg)).collect();
    // OR: joined to the last condition of the same kind (#tags with #tags, ckpt: with ckpt: ...);
    // with none, it is a new condition (filters of other kinds still narrow)
    let kind = word_kind(item);
    let last = v.iter().rposition(|w| !w.starts_with('-') && word_kind(w) == kind);
    match last {
        Some(k) if or => {
            v[k].push('|');
            v[k].push_str(item);
        }
        _ => v.push(item.to_string()),
    }
    join_words(v)
}

/// What kind of condition a word is: "#", a key ("ckpt", "album" ...) or "" for a prompt tag.
fn word_kind(w: &str) -> String {
    let first = w.split('|').next().unwrap_or("").trim_start_matches('-').to_lowercase();
    if first.starts_with('#') {
        return "#".into();
    }
    match first.split_once([':', '=', '<', '>']) {
        Some((k, _)) if KEYS.contains(&k) => if k == "앨범" { "album".into() } else { k.to_string() },
        _ => String::new(),
    }
}

/// The word still being typed at the end of the search (shown as a hint), if it is
/// a prompt tag or hashtag that is matched as a beginning.
pub fn typing_word(s: &str) -> Option<String> {
    if s.trim().is_empty() || s.trim_end().ends_with([',', '|']) {
        return None;
    }
    let w = split_words(s).pop()?;
    let last = w.rsplit('|').next()?.to_string();
    let any = parse_atoms(&last);
    (!w.starts_with('-') && matches!(any.as_slice(), [Atom::Text { .. }] | [Atom::Tag(_)])).then_some(last)
}

/// Kept searches (saved filters, recent searches) are written in AND form (`explicit`):
/// true when picking one should turn the switch to AND (it has commas between tags).
pub fn wants_and(q: &str) -> bool {
    split_words(q)
        .iter()
        .filter(|w| {
            let neg = w.starts_with('-') && w.len() > 1;
            let any: Vec<Atom> = w.split('|').flat_map(parse_atoms).collect();
            tag_like(neg, &any)
        })
        .count()
        >= 2
}

/// The search with hashtag #old renamed #new (also inside a|b and after '-').
pub fn rename_hashtag(q: &str, old: &str, new: &str) -> String {
    let (o, n) = (format!("#{}", old.to_lowercase()), format!("#{new}"));
    let words: Vec<String> = split_words(q)
        .into_iter()
        .map(|w| {
            let (neg, body) = match w.strip_prefix('-') {
                Some(b) => ("-", b.to_string()),
                None => ("", w.clone()),
            };
            let parts: Vec<String> = body.split('|').map(|p| if p.to_lowercase() == o { n.clone() } else { p.to_string() }).collect();
            format!("{neg}{}", parts.join("|"))
        })
        .collect();
    join_words(words)
}

/// Ctrl+click: this item as the only condition of its kind (`key`: "#", "ckpt:", "album:" ...).
pub fn only_item(q: &str, item: &str, key: &str) -> String {
    let lk = key.to_lowercase();
    let mut v: Vec<String> = split_words(q)
        .into_iter()
        .filter_map(|w| {
            let parts: Vec<&str> = w.split('|').filter(|p| !p.trim_start_matches('-').to_lowercase().starts_with(&lk)).collect();
            let neg = w.starts_with('-') && !parts.is_empty() && !parts[0].starts_with('-');
            let s = parts.join("|");
            (!parts.is_empty()).then(|| if neg { format!("-{s}") } else { s })
        })
        .collect();
    v.push(item.to_string());
    join_words(v)
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
        .join(", ")
}

/// Does the search name an album at all (also negated or unknown ones)?
pub fn names_album(q: &str) -> bool {
    split_words(q).iter().any(|w| {
        let l = w.trim_start_matches('-').to_lowercase();
        l.starts_with("album:") || l.starts_with("앨범:")
    })
}

/// Values of the (non-negated) `key:a|key:b` word, keeping the letters' case (album names).
pub fn group_values_cased(q: &str, key: &str) -> Vec<String> {
    let pre = format!("{key}:");
    for w in split_words(q) {
        if w.to_lowercase().starts_with(&pre) {
            return w.split('|').filter(|p| p.to_lowercase().starts_with(&pre)).map(|p| p[pre.len()..].to_string()).collect();
        }
    }
    Vec::new()
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
        assert_eq!(toggle_word("a", "#x"), "a, #x");
        assert_eq!(toggle_word("a -#x", "#x"), "a, #x");
        assert_eq!(set_prefixed("a score>=3", "score", Some("score:5")), "a, score:5");
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
        assert_eq!(without_album("ckpt:x album:3 #a"), "ckpt:x, #a");
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
        // album 0 = also at the top: shown at the top and in its album, not 무소속
        i.albums = vec![5, 0];
        assert!(Query::parse("album:top").matches(&i));
        assert!(Query::parse("album:5").matches(&i));
        assert!(!Query::parse("album:none").matches(&i));
        i.albums = vec![0];
        assert!(Query::parse("album:none").matches(&i));
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
        assert_eq!(toggle_word("\"album:qz test\"", "#a"), "\"album:qz test\", #a");
        assert_eq!(rename_album_words("x album:41+|album:none -album:qztrip", |id| format!("n{id}")), "x, album:n41+|album:none, -album:n41");
        assert_eq!(group_values_cased("\"album:Qz test+|album:none\"", "album"), vec!["Qz test+", "none"]);
    }

    #[test]
    fn commas_and_whole_tags() {
        let w = |q: &str| split_words(q);
        assert_eq!(w("long hair, scar, #a"), ["long hair", "scar", "#a"]);
        assert_eq!(w("long hair #a score>=4 red eyes"), ["long hair", "#a", "score>=4", "red eyes"]);
        assert_eq!(w("#a | #b"), ["#a|#b"]);
        assert_eq!(w("#a |#b, c"), ["#a|#b", "c"]);
        assert_eq!(w("long hair | short hair"), ["long hair|short hair"]);
        assert_eq!(w("red -blue eyes"), ["red", "-blue eyes"]);
        assert_eq!(w("\"album:my trip\" x"), ["album:my trip", "x"]);
        assert_eq!(w("album:x+ #a #b"), ["album:x+", "#a", "#b"]);
        let mut i = img();
        i.prompt = "1girl, (Long_Hair:1.2), facial scar, scar on cheek, \\(artist\\), scarf\nblue eyes".into();
        let m = |q: &str| Query::parse(q).matches(&i);
        assert!(m("long hair") && m("LONG_HAIR") && m("long hair, blue eyes"));
        assert!(!m("long") && !m("hair"), "a part of a tag is not the tag");
        assert!(!m("scar"), "no tag is exactly scar");
        assert!(m("__scar") && m("scar__") && m("__scar__") && m("artist"));
        let mut i = i.clone();
        i.prompt = "scarf, 1girl".into();
        let m = |q: &str| Query::parse(q).matches(&i);
        assert!(!m("__scar") && m("scar__") && m("__scar__"));
        assert!(m("#pixiv | #nope") && m("#nope|#alice") && !m("#nope, #alice"));
        assert!(m("pixiv") && m("animea"), "hashtags and the preset by name");
        assert!(!m("__") && !m("____"));
        assert!(m("-white hair") && !m("-1girl"));
    }

    #[test]
    fn sentence_prompt_tags() {
        let parts: Vec<String> = prompt_parts("1girl, solo. A girl. The girl has blue eyes.\njojo pose, (arm up:1.2), flexing.").map(norm_tag).filter(|t| !t.is_empty()).collect();
        assert_eq!(parts, ["1girl", "solo", "a girl", "the girl has blue eyes", "jojo pose", "arm up", "flexing"]);
        let mut i = img();
        i.prompt = "anima, 1girl, solo. A girl. The girl has multicolored hair.\n jojo pose, martial arts, flexing.".into();
        for q in ["1girl, solo", "jojo pose", "flexing", "solo, jojo_pose"] {
            assert!(Query::parse(q).matches(&i), "{q}");
        }
        assert!(!Query::parse("girl").matches(&i));
    }

    #[test]
    fn or_mode_and_typing() {
        let mut i = img();
        i.prompt = "1girl, solo, frills".into();
        i.tags = vec!["pixiv".into()];
        let m = |q: &str, or: bool, live: bool| Query::parse_with(q, or, live).matches(&i);
        // typing: the last word is a beginning
        assert!(!m("1girl, so", false, false));
        assert!(m("1girl, so", false, true));
        assert!(m("1girl, sol", false, true));
        assert!(!m("1girl, so, ", false, true));
        assert!(m("1girl, -so", false, true));
        assert!(m("1girl, -", false, true));
        assert!(m("long ha", false, true) == false);
        // OR mode: commas between tags mean either, filters still narrow
        assert!(!m("frills, ribbon", false, false));
        assert!(m("frills, ribbon", true, false));
        assert!(m("ribbon, #pixiv", true, false));
        assert!(!m("ribbon, bow", true, false));
        assert!(!m("frills, ribbon, score>=5", true, false));
        assert!(m("frills, ribbon, score>=3", true, false));
        assert!(!m("frills, ribbon, -solo", true, false));
        assert!(m("ribbon, fri", true, true));
        // kept searches spell OR out
        assert_eq!(explicit("frills, ribbon, score>=3", true), "frills|ribbon, score>=3");
        assert_eq!(explicit("frills, ribbon", false), "frills, ribbon");
        assert_eq!(explicit("frills, -solo", true), "frills, -solo");
        assert!(Query::parse(&explicit("frills, ribbon", true)).matches(&i));
    }

    #[test]
    fn typing_more_and_kinds() {
        let mut i = img();
        i.prompt = "1girl, long hair".into();
        let m = |q: &str, or: bool, live: bool| Query::parse_with(q, or, live).matches(&i);
        assert!(m("1girl, long ", false, true));
        assert!(m("1girl, long h", false, true));
        assert!(m("#pix", false, true) && !m("#pix", false, false));
        assert!(m("#pix, ", false, true) == false);
        // OR clicks join the same kind only
        assert_eq!(click_item("1girl, solo", "ckpt:x", &[], true), "1girl, solo, ckpt:x");
        assert_eq!(click_item("score>=4", "#p", &[], true), "score>=4, #p");
        assert_eq!(click_item("#a, score>=4", "#p", &[], true), "#a|#p, score>=4");
        assert_eq!(click_item("ckpt:a, #b", "ckpt:x", &[], true), "ckpt:a|ckpt:x, #b");
        assert_eq!(click_item("ckpt:a", "ckpt:x", &[], false), "ckpt:a, ckpt:x");
        assert!(wants_and("a, b, score>=3") && !wants_and("a|b, score>=3"));
        assert_eq!(rename_hashtag("#old, x|#Old, -#old", "old", "new"), "#new, x|#new, -#new");
    }

    #[test]
    fn clicks_and_or() {
        assert_eq!(click_item("", "#a", &[], false), "#a");
        assert_eq!(click_item("#a", "#b", &[], false), "#a, #b");
        assert_eq!(click_item("#a", "#b", &[], true), "#a|#b");
        assert_eq!(click_item("#a|#b", "ckpt:x", &[], true), "#a|#b, ckpt:x");
        assert_eq!(click_item("#a|#b, long hair", "#b", &[], true), "#a, long hair");
        assert_eq!(click_item("#a, sort:new", "#b", &[], true), "#a|#b, sort:new");
        assert_eq!(click_item("#a, -#c", "#b", &[], true), "#a|#b, -#c");
        assert_eq!(click_item("-#b", "#b", &[], false), "#b");
        assert_eq!(click_item("album:trip", "album:trip+", &["album:trip"], true), "");
        assert!(has_item("x|#A", &["#a"]) && !has_item("-#a", &["#a"]));
        assert_eq!(only_item("#a|ckpt:y, #b, ckpt:z, long hair", "ckpt:x", "ckpt:"), "#a, #b, long hair, ckpt:x");
        assert_eq!(only_item("#a, #b", "#c", "#"), "#c");
    }
}
