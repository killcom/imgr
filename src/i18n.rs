//! Screen language: Korean or English (⚙ 설정 › 언어 / Language).
//!
//! Every text on screen is written with both languages side by side:
//!   tr("정하기", "OK")                         -> &'static str
//!   trf!("{n}장 고름", "{n} selected")         -> String (format! for each)
//! The Korean text is the original; the English one is shown when the
//! language is "en". Data (query words, file names, tags) is never translated.
use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

/// Is the screen in English?
pub fn en() -> bool {
    EN.load(Ordering::Relaxed)
}

/// "ko" or "en" (anything else counts as Korean).
pub fn set(lang: &str) {
    EN.store(lang == "en", Ordering::Relaxed);
}

/// The text for the current language.
pub fn tr(ko: &'static str, en: &'static str) -> &'static str {
    if self::en() { en } else { ko }
}

/// format! in the current language: trf!("{n}장", "{n} pictures") or with
/// positional arguments trf!("{}장", "{} pictures", n). Both texts must use
/// the same arguments.
#[macro_export]
macro_rules! trf {
    ($ko:literal, $en:literal $(,)?) => {
        if $crate::i18n::en() { format!($en) } else { format!($ko) }
    };
    ($ko:literal, $en:literal, $($arg:tt)*) => {
        if $crate::i18n::en() { format!($en, $($arg)*) } else { format!($ko, $($arg)*) }
    };
}

/// First start: Korean if Windows itself is in Korean, else English.
pub fn system_lang() -> String {
    // LANGID primary language 0x12 = Korean
    let id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    if id & 0x3ff == 0x12 { "ko".into() } else { "en".into() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn switch() {
        super::set("en");
        assert_eq!(super::tr("가", "a"), "a");
        let n = 3;
        assert_eq!(trf!("{n}장", "{n} pics"), "3 pics");
        assert_eq!(trf!("{}장", "{} pics", n), "3 pics");
        super::set("ko");
        assert_eq!(super::tr("가", "a"), "가");
        assert_eq!(trf!("{}장", "{} pics", n), "3장");
    }
}
