//! Language of user-facing messages: English (default) or Brazilian
//! Portuguese.
//!
//! Every message keeps both versions side by side at the call site:
//! `tr!("English", "Português")` returns a `&'static str` and
//! `trf!("English {x}", "Português {x}")` formats a `String`. The desktop app
//! picks the language (system locale or the user's choice); the CLI never
//! changes it, so it always speaks English.

use std::sync::atomic::{AtomicU8, Ordering};

/// Language of the messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    /// English (default).
    #[default]
    En,
    /// Brazilian Portuguese.
    Pt,
}

impl Lang {
    /// Language for a POSIX locale or BCP 47 tag (`pt_BR.UTF-8`, `pt-BR`,
    /// `pt`): Portuguese for `pt*`, English for everything else.
    #[must_use]
    pub fn from_locale(locale: &str) -> Self {
        let lower = locale.trim().to_ascii_lowercase();
        if lower == "pt"
            || lower.starts_with("pt_")
            || lower.starts_with("pt-")
            || lower.starts_with("pt.")
        {
            Self::Pt
        } else {
            Self::En
        }
    }

    /// Language of the session, from `LC_ALL`, `LC_MESSAGES` and `LANG`
    /// (the first one set wins, as in POSIX).
    #[must_use]
    pub fn from_env() -> Self {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()))
            .map_or(Self::En, |locale| Self::from_locale(&locale))
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

/// Changes the language of the messages built from now on.
pub fn set_lang(lang: Lang) {
    LANG.store(
        match lang {
            Lang::En => 0,
            Lang::Pt => 1,
        },
        Ordering::Relaxed,
    );
}

/// Current language.
#[must_use]
pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 {
        Lang::Pt
    } else {
        Lang::En
    }
}

/// Message in the current language (`&'static str`).
#[macro_export]
macro_rules! tr {
    ($en:literal, $pt:literal $(,)?) => {
        match $crate::i18n::lang() {
            $crate::i18n::Lang::En => $en,
            $crate::i18n::Lang::Pt => $pt,
        }
    };
}

/// Formatted message in the current language (`String`). Accepts implicit
/// captures (`{name}`) and explicit arguments, like `format!`.
#[macro_export]
macro_rules! trf {
    ($en:literal, $pt:literal $(,)?) => {
        match $crate::i18n::lang() {
            $crate::i18n::Lang::En => format!($en),
            $crate::i18n::Lang::Pt => format!($pt),
        }
    };
    ($en:literal, $pt:literal, $($args:tt)+) => {
        match $crate::i18n::lang() {
            $crate::i18n::Lang::En => format!($en, $($args)+),
            $crate::i18n::Lang::Pt => format!($pt, $($args)+),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales() {
        for pt in ["pt_BR.UTF-8", "pt-BR", "pt", "PT_pt", "pt.UTF-8"] {
            assert_eq!(Lang::from_locale(pt), Lang::Pt, "{pt}");
        }
        for en in ["en_US.UTF-8", "C", "POSIX", "", "es_ES", "ptx"] {
            assert_eq!(Lang::from_locale(en), Lang::En, "{en}");
        }
    }

    #[test]
    fn default_is_english() {
        // Tests run in English: nobody calls set_lang in this binary
        // (Portuguese is covered in tests/i18n_pt.rs, its own process).
        assert_eq!(lang(), Lang::En);
        assert_eq!(tr!("hello", "olá"), "hello");
        let name = "x";
        assert_eq!(trf!("hi {name}", "oi {name}"), "hi x");
        assert_eq!(trf!("{} + {}", "{} e {}", 1, 2), "1 + 2");
    }
}
