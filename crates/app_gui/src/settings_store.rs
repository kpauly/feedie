//! Persistence for user settings such as language preference.

use crate::i18n::LanguagePreference;
use directories_next::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppSettings {
    pub(crate) language: LanguagePreference,
    pub(crate) background_labels: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: LanguagePreference::System,
            background_labels: vec!["achtergrond".to_string()],
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    ProjectDirs::from("nl", "Feedie", "Feedie").map(|dirs| dirs.data_dir().join("settings.json"))
}

pub(crate) fn load_settings() -> AppSettings {
    let Some(path) = settings_path() else {
        return AppSettings::default();
    };
    let Ok(contents) = fs::read_to_string(&path) else {
        return AppSettings::default();
    };
    match serde_json::from_str::<AppSettings>(&contents) {
        Ok(settings) => settings,
        Err(err) => {
            tracing::warn!("Instellingenbestand onleesbaar: {err}");
            AppSettings::default()
        }
    }
}

pub(crate) fn save_settings(settings: &AppSettings) -> anyhow::Result<()> {
    let Some(path) = settings_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = serde_json::to_string_pretty(settings)?;
    fs::write(path, payload)?;
    Ok(())
}
