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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn collect_keys(dir: &Path) -> std::io::Result<BTreeSet<String>> {
        let mut keys = BTreeSet::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("ftl") {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            for line in content.lines() {
                let trimmed = line.trim_start();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((key, _)) = trimmed.split_once('=') {
                    let key = key.trim();
                    if key.is_empty() || key.starts_with('.') {
                        continue;
                    }
                    keys.insert(key.to_string());
                }
            }
        }
        Ok(keys)
    }

    #[test]
    fn fluent_keys_match_across_locales() {
        let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("i18n");
        let base_locale = "en-US";
        let base_path = base_dir.join(base_locale);
        assert!(
            base_path.is_dir(),
            "Missing base locale directory: {}",
            base_path.display()
        );
        let base_keys = collect_keys(&base_path).expect("read base locale keys");
        let mut failures = Vec::new();
        for entry in fs::read_dir(&base_dir).expect("read i18n dir") {
            let entry = entry.expect("read locale entry");
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let locale = entry.file_name().to_string_lossy().to_string();
            if locale == base_locale {
                continue;
            }
            let keys = collect_keys(&path).expect("read locale keys");
            let missing: Vec<_> = base_keys.difference(&keys).cloned().collect();
            let extra: Vec<_> = keys.difference(&base_keys).cloned().collect();
            if !missing.is_empty() || !extra.is_empty() {
                let mut message = format!("locale {locale}");
                if !missing.is_empty() {
                    message.push_str(&format!(", missing: {}", missing.join(", ")));
                }
                if !extra.is_empty() {
                    message.push_str(&format!(", extra: {}", extra.join(", ")));
                }
                failures.push(message);
            }
        }
        assert!(
            failures.is_empty(),
            "Fluent locale key mismatch:\n{}",
            failures.join("\n")
        );
    }
}
