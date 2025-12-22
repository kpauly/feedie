//! Language selection and Fluent helpers.

use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::{Loader, static_loader};
use i18n_embed::DesktopLanguageRequester;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use unic_langid::LanguageIdentifier;

static_loader! {
    static LOCALES = {
        locales: "i18n",
        fallback_language: "en-US",
    };
}

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

impl Language {
    pub fn id(self) -> LanguageIdentifier {
        match self {
            Language::Dutch => "nl-NL".parse().expect("valid langid"),
            Language::English => "en-US".parse().expect("valid langid"),
        }
    }
}

pub fn detect_system_language() -> Language {
    let requested = DesktopLanguageRequester::requested_languages();
    if requested
        .iter()
        .any(|lang| lang.to_string().to_ascii_lowercase().starts_with("nl"))
    {
        Language::Dutch
    } else {
        Language::English
    }
}

pub fn t_for(language: Language, key: &str) -> String {
    LOCALES.lookup(&language.id(), key)
}

pub type Args = HashMap<Cow<'static, str>, FluentValue<'static>>;

pub fn t_for_args(language: Language, key: &str, args: &Args) -> String {
    LOCALES.lookup_with_args(&language.id(), key, args)
}
