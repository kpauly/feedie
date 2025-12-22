//! Model installation helpers and confidence heuristics.

use crate::app::{
    LABEL_FILE_NAME, LabelOption, MODEL_FILE_NAME, SOMETHING_LABEL, UiApp, VERSION_FILE_NAME,
};
use crate::util::canonical_label;
use anyhow::{Context, anyhow};
use directories_next::ProjectDirs;
use feeder_core::{ClassifierConfig, Decision, ImageInfo};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

impl UiApp {
    /// Recomputes the `present` flag for every row based on the current threshold.
    pub(crate) fn apply_presence_threshold(&mut self) {
        let threshold = self.presence_threshold;
        let backgrounds = &self.background_labels;
        for info in &mut self.rijen {
            let mut present = false;
            if let Some(classification) = &info.classification {
                match &classification.decision {
                    Decision::Label(name) => {
                        let canonical = canonical_label(name);
                        if !backgrounds.iter().any(|bg| bg == &canonical) {
                            present = classification.confidence >= threshold;
                        }
                    }
                    Decision::Unknown => present = false,
                }
            }
            info.present = present;
        }
    }

    /// Returns true when the provided canonical label is considered background.
    pub(crate) fn is_background_label(&self, name_lower: &str) -> bool {
        self.background_labels.iter().any(|bg| bg == name_lower)
    }

    /// Determines whether a classification should be treated as uncertain.
    pub(crate) fn is_onzeker(&self, info: &ImageInfo) -> bool {
        let Some(classification) = &info.classification else {
            return false;
        };
        match &classification.decision {
            Decision::Label(name) => {
                let canonical = canonical_label(name);
                if canonical == SOMETHING_LABEL {
                    return !self.is_background_label(&canonical);
                }
                if self.is_background_label(&canonical) {
                    return false;
                }
                classification.confidence < self.presence_threshold
            }
            Decision::Unknown => false,
        }
    }

    /// Builds the classifier configuration for the next scan job.
    pub(crate) fn classifier_config(&self) -> ClassifierConfig {
        ClassifierConfig {
            model_path: self.model_file_path(),
            labels_path: self.labels_path(),
            presence_threshold: self.pending_presence_threshold,
            batch_size: self.batch_size.max(1),
            background_labels: self.background_labels.clone(),
            ..Default::default()
        }
    }

    /// Points to the on-disk EfficientViT model weights.
    pub(crate) fn model_file_path(&self) -> PathBuf {
        self.model_root.join(MODEL_FILE_NAME)
    }

    /// Points to the CSV file containing all known labels.
    pub(crate) fn labels_path(&self) -> PathBuf {
        self.model_root.join(LABEL_FILE_NAME)
    }

    /// Points to the file where the downloaded model version is stored.
    pub(crate) fn model_version_path(&self) -> PathBuf {
        self.model_root.join(VERSION_FILE_NAME)
    }

    /// Loads label metadata from disk and filters duplicates.
    pub(crate) fn load_label_options_from(path: &Path) -> Vec<LabelOption> {
        let Ok(content) = std::fs::read_to_string(path) else {
            tracing::warn!(
                "Kon labels niet laden uit {}: bestand ontbreekt of is onleesbaar",
                path.display()
            );
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut options = Vec::new();
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(content.as_bytes());
        for record in reader.records().flatten() {
            let display = record.get(0).unwrap_or_default().trim();
            if display.is_empty() {
                continue;
            }
            let canonical = canonical_label(display);
            if canonical.is_empty() || !seen.insert(canonical.clone()) {
                continue;
            }
            let display_en = if record.len() >= 3 {
                let raw = record.get(1).unwrap_or_default().trim();
                if raw.is_empty() {
                    None
                } else {
                    Some(raw.to_string())
                }
            } else {
                None
            };
            let scientific_raw = if record.len() >= 3 {
                record
                    .iter()
                    .skip(2)
                    .collect::<Vec<_>>()
                    .join(",")
                    .trim()
                    .to_string()
            } else {
                record.get(1).unwrap_or_default().trim().to_string()
            };
            options.push(LabelOption {
                canonical,
                display: display.to_string(),
                display_en,
                scientific: if scientific_raw.is_empty() {
                    None
                } else {
                    Some(scientific_raw)
                },
            });
        }
        if options.iter().any(|option| option.display_en.is_none()) {
            let fallback_path = bundled_models_dir().join(LABEL_FILE_NAME);
            if fallback_path != path
                && let Some(english_map) = load_english_labels_from(&fallback_path)
            {
                for option in &mut options {
                    if option.display_en.is_none()
                        && let Some(display_en) = english_map.get(&option.canonical)
                    {
                        option.display_en = Some(display_en.clone());
                    }
                }
            }
        }
        options
    }

    /// Ensures the model directory exists and returns its path and version.
    pub(crate) fn prepare_model_dir() -> (PathBuf, String) {
        if let Some(proj_dirs) = ProjectDirs::from("nl", "Feedie", "Feedie") {
            let models_dir = proj_dirs.data_dir().join("models");
            match Self::ensure_models_present(&models_dir) {
                Ok(()) => {
                    let version = read_model_version_from(&models_dir.join(VERSION_FILE_NAME));
                    return (models_dir, version);
                }
                Err(err) => {
                    tracing::warn!("Kon modelmap niet voorbereiden in AppData: {err}");
                }
            }
        }
        let bundled = bundled_models_dir();
        let version = read_model_version_from(&bundled.join(VERSION_FILE_NAME));
        (bundled, version)
    }

    /// Copies the bundled models into the writable directory when missing.
    pub(crate) fn ensure_models_present(target: &Path) -> anyhow::Result<()> {
        if target.exists() {
            return Ok(());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Kon map {} niet aanmaken", parent.display()))?;
        }
        copy_dir_recursive(&bundled_models_dir(), target)
    }
}

/// Resolves the path that contains the bundled models that ship with the app.
fn bundled_models_dir() -> PathBuf {
    PathBuf::from("models")
}

fn load_english_labels_from(path: &Path) -> Option<HashMap<String, String>> {
    let Ok(content) = fs::read_to_string(path) else {
        return None;
    };
    let mut map = HashMap::new();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(content.as_bytes());
    for record in reader.records().flatten() {
        if record.len() < 3 {
            continue;
        }
        let display = record.get(0).unwrap_or_default().trim();
        if display.is_empty() {
            continue;
        }
        let canonical = canonical_label(display);
        if canonical.is_empty() {
            continue;
        }
        let display_en = record.get(1).unwrap_or_default().trim();
        if display_en.is_empty() {
            continue;
        }
        map.entry(canonical)
            .or_insert_with(|| display_en.to_string());
    }
    if map.is_empty() { None } else { Some(map) }
}

/// Recursively copies the bundled model files into the writable directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    if !src.exists() {
        return Err(anyhow!("Bronmodelmap ontbreekt: {}", src.to_string_lossy()));
    }
    fs::create_dir_all(dst).with_context(|| format!("Kon map {} niet aanmaken", dst.display()))?;
    for entry in
        fs::read_dir(src).with_context(|| format!("{} kan niet worden gelezen", src.display()))?
    {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), &dest_path).with_context(|| {
                format!(
                    "Kopi\u{00EB}ren van {} naar {} mislukt",
                    entry.path().display(),
                    dest_path.display()
                )
            })?;
        }
    }
    Ok(())
}

/// Reads and normalizes the persisted model version string.
pub(crate) fn read_model_version_from(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let normalized = normalize_model_version(content.trim());
            if normalized.is_empty() {
                "onbekend".to_string()
            } else {
                normalized
            }
        }
        Err(err) => {
            tracing::warn!("Kon modelversie niet lezen uit {}: {err}", path.display());
            "onbekend".to_string()
        }
    }
}

/// Normalizes various version formats to a consistent one.
pub(crate) fn normalize_model_version(value: &str) -> String {
    let trimmed = value.trim();
    let without_prefix = trimmed
        .strip_prefix("model-")
        .or_else(|| trimmed.strip_prefix("MODEL-"))
        .unwrap_or(trimmed);
    let without_v = without_prefix
        .strip_prefix('v')
        .or_else(|| without_prefix.strip_prefix('V'))
        .unwrap_or(without_prefix);
    without_v.to_string()
}
