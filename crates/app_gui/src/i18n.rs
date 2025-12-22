//! Language selection and system locale helpers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LanguagePreference {
    System,
    Dutch,
    English,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Dutch,
    English,
}

impl LanguagePreference {
    pub fn resolve(self) -> Language {
        match self {
            LanguagePreference::System => detect_system_language(),
            LanguagePreference::Dutch => Language::Dutch,
            LanguagePreference::English => Language::English,
        }
    }
}

pub fn detect_system_language() -> Language {
    match sys_locale::get_locale() {
        Some(locale) => {
            let lower = locale.to_ascii_lowercase();
            if lower.starts_with("nl") {
                Language::Dutch
            } else {
                Language::English
            }
        }
        None => Language::English,
    }
}

pub fn tr_for(language: Language, nl: &'static str, en: &'static str) -> &'static str {
    match language {
        Language::Dutch => nl,
        Language::English => en,
    }
}
