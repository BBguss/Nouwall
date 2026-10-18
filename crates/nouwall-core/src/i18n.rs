//! English and Indonesian text for everything a user reads (warnings, tier
//! reasons, tray menu, status lines). Log files stay in English.
//!
//! The active language is process-wide: set it once from the config with
//! [`set`] and pick strings with the [`tr!`](crate::tr) macro.

use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

/// The `language` setting in `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Follow the Windows display language.
    #[default]
    Auto,
    En,
    Id,
}

/// A resolved language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Id,
}

impl Language {
    /// Resolve `Auto` against the system language tag (`"id-ID"`, `"en-US"`, ...).
    /// Anything that is not Indonesian falls back to English.
    pub fn resolve(self, system: &str) -> Lang {
        match self {
            Language::En => Lang::En,
            Language::Id => Lang::Id,
            Language::Auto => {
                let tag = system.to_ascii_lowercase();
                // "in" is the legacy ISO 639 code for Indonesian.
                if tag == "id" || tag == "in" || tag.starts_with("id-") || tag.starts_with("in-") {
                    Lang::Id
                } else {
                    Lang::En
                }
            }
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == Lang::Id as u8 {
        Lang::Id
    } else {
        Lang::En
    }
}

/// `format!` with an English and an Indonesian template, chosen by [`current`].
///
/// ```
/// use nouwall_core::tr;
/// let n = 3;
/// assert_eq!(tr!("{n} monitors", "{n} monitor"), "3 monitors");
/// ```
#[macro_export]
macro_rules! tr {
    ($en:literal, $id:literal $(, $arg:expr)* $(,)?) => {
        match $crate::i18n::current() {
            $crate::i18n::Lang::Id => format!($id $(, $arg)*),
            $crate::i18n::Lang::En => format!($en $(, $arg)*),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_system_language() {
        assert_eq!(Language::Auto.resolve("id-ID"), Lang::Id);
        assert_eq!(Language::Auto.resolve("in"), Lang::Id);
        assert_eq!(Language::Auto.resolve("en-US"), Lang::En);
        assert_eq!(Language::Auto.resolve("ja-JP"), Lang::En);
        assert_eq!(Language::Auto.resolve(""), Lang::En);
        assert_eq!(Language::Id.resolve("en-US"), Lang::Id);
        assert_eq!(Language::En.resolve("id-ID"), Lang::En);
    }

    #[test]
    fn setting_parses_lowercase() {
        #[derive(Deserialize)]
        struct T {
            language: Language,
        }
        let t: T = toml::from_str("language = \"id\"").unwrap();
        assert_eq!(t.language, Language::Id);
    }
}
