//! Cached scan persistence keyed by folder path.

use crate::app::UiApp;
use anyhow::Context;
use directories_next::ProjectDirs;
use feeder_core::{Classification, ImageInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize)]
struct CachedFile {
    rel_path: String,
    size: u64,
    modified: u64,
    present: bool,
    classification: Option<Classification>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedScan {
    generated_at: u64,
    model_version: String,
    files: Vec<CachedFile>,
    total_files: usize,
}

fn cache_dir() -> Option<PathBuf> {
    ProjectDirs::from("nl", "Feedie", "Feedie").map(|dirs| dirs.data_dir().join("cache"))
}

fn cache_path_for_folder(folder: &Path) -> Option<PathBuf> {
    let dir = cache_dir()?;
    let canonical = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.to_string_lossy().hash(&mut hasher);
    let hash = format!("{:x}", hasher.finish());
    Some(dir.join(format!("{hash}.json")))
}

fn file_signature(path: &Path) -> Option<(u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    let size = meta.len();
    let modified = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((size, modified))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl UiApp {
    pub(crate) fn try_load_cached_scan(&mut self, folder: &Path) -> anyhow::Result<bool> {
        let Some(cache_file) = cache_path_for_folder(folder) else {
            return Ok(false);
        };
        if !cache_file.exists() {
            return Ok(false);
        }
        let data = fs::read_to_string(&cache_file)
            .with_context(|| format!("Cannot read cache {}", cache_file.display()))?;
        let cached: CachedScan =
            serde_json::from_str(&data).with_context(|| "Corrupt cache file")?;

        // Build current file signatures.
        let rows = feeder_core::scan_folder_with(folder, feeder_core::ScanOptions::default())
            .with_context(|| "Failed to list folder while validating cache")?;
        let mut current: HashMap<String, (PathBuf, u64, u64)> = HashMap::new();
        for info in rows {
            if let Some((size, modified)) = file_signature(&info.file)
                && let Ok(rel) = info.file.strip_prefix(folder)
            {
                current.insert(
                    rel.to_string_lossy().to_string(),
                    (info.file.clone(), size, modified),
                );
            }
        }

        if current.len() != cached.files.len() || current.len() != cached.total_files {
            return Ok(false);
        }

        // Validate signatures.
        let mut rebuilt: Vec<ImageInfo> = Vec::with_capacity(cached.files.len());
        for entry in &cached.files {
            let Some((abs, size, modified)) = current.get(&entry.rel_path).cloned() else {
                return Ok(false);
            };
            if size != entry.size || modified != entry.modified {
                return Ok(false);
            }
            rebuilt.push(ImageInfo {
                file: abs,
                present: entry.present,
                classification: entry.classification.clone(),
            });
        }

        self.rijen = rebuilt;
        self.total_files = cached.total_files;
        self.has_scanned = true;
        self.scan_in_progress = false;
        self.current_page = 0;
        self.status = format!(
            "{} ({})",
            self.tr("Gereed: cache geladen", "Done: cache loaded"),
            cached.model_version
        );
        self.reset_thumbnail_cache();
        self.full_images.clear();
        self.full_keys.clear();
        self.reset_selection();
        Ok(true)
    }

    pub(crate) fn save_cache_for_current_folder(&mut self) {
        let Some(folder) = &self.gekozen_map else {
            return;
        };
        if self.rijen.is_empty() {
            return;
        }
        let Some(cache_file) = cache_path_for_folder(folder) else {
            return;
        };
        if let Some(dir) = cache_file.parent()
            && let Err(err) = fs::create_dir_all(dir)
        {
            tracing::warn!("Could not create cache dir {}: {err}", dir.display());
            return;
        }

        let mut files: Vec<CachedFile> = Vec::new();
        for info in &self.rijen {
            let Ok(rel_path) = info.file.strip_prefix(folder) else {
                continue;
            };
            let Some((size, modified)) = file_signature(&info.file) else {
                continue;
            };
            files.push(CachedFile {
                rel_path: rel_path.to_string_lossy().to_string(),
                size,
                modified,
                present: info.present,
                classification: info.classification.clone(),
            });
        }

        let payload = CachedScan {
            generated_at: now_secs(),
            model_version: self.model_version.clone(),
            total_files: files.len(),
            files,
        };

        match serde_json::to_string(&payload) {
            Ok(json) => {
                if let Err(err) = fs::write(&cache_file, json) {
                    tracing::warn!("Cache write failed {}: {err}", cache_file.display());
                }
            }
            Err(err) => tracing::warn!("Cache serialization failed: {err}"),
        }
    }
}
