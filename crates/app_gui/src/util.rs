//! Utility helpers for label formatting, filenames, and icons.

use anyhow::{Context, anyhow};
use chrono::{DateTime, Local};
use eframe::egui::viewport::IconData;
use std::fs;
use std::path::{Path, PathBuf};

/// Normalizes labels by stripping Feedie suffixes and converting to lowercase.
///
/// # Examples
///
/// ```
/// let canonical = feedie::util::canonical_label("Koolmees (manueel)");
/// assert_eq!("koolmees", canonical);
/// ```
pub fn canonical_label(name: &str) -> String {
    let stripped = name.strip_suffix(" (manueel)").unwrap_or(name).trim();
    let primary = stripped
        .split_once(',')
        .map(|(first, _)| first.trim())
        .unwrap_or(stripped);
    let cleaned = primary.trim_end_matches(['.', ',']).trim();
    cleaned.to_ascii_lowercase()
}

/// Converts machine friendly names into a readable display label.
///
/// # Examples
///
/// ```
/// assert_eq!("Bos Muis", feedie::util::fallback_display_label("bos muis"));
/// ```
pub fn fallback_display_label(name: &str) -> String {
    let mut result = String::new();
    let mut capitalize = true;
    for ch in name.chars() {
        if ch.is_whitespace() {
            capitalize = true;
            result.push(ch);
            continue;
        }
        if capitalize {
            result.extend(ch.to_uppercase());
            capitalize = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Ensures filenames are filesystem-safe by removing dangerous characters.
///
/// # Panics
///
/// This function never panics.
pub fn sanitize_for_path(input: &str) -> String {
    let mut sanitized = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            sanitized.push('_');
        } else {
            sanitized.push(ch);
        }
    }
    sanitized.trim().trim_matches('.').to_string()
}

/// Generates a unique path within `base_dir` using the provided file stem and extension.
///
/// # Examples
///
/// ```
/// # use std::path::Path;
/// let dir = tempfile::tempdir().unwrap();
/// let path = feedie::util::next_available_export_path(dir.path(), "result", "jpg");
/// assert!(path.ends_with("result.jpg"));
/// ```
pub fn next_available_export_path(base_dir: &Path, base: &str, ext: &str) -> PathBuf {
    let mut attempt = 0usize;
    loop {
        let filename = if attempt == 0 {
            format!("{base}.{ext}")
        } else {
            format!("{base} ({}).{ext}", attempt + 1)
        };
        let candidate = base_dir.join(&filename);
        if !candidate.exists() {
            return candidate;
        }
        attempt += 1;
    }
}

/// Derives human readable timestamps from a file's metadata.
///
/// # Errors
///
/// Returns an error if the metadata or timestamps cannot be read from disk.
pub fn extract_timestamp(path: &Path) -> anyhow::Result<(String, String)> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("Kon metadata niet lezen voor {}", path.display()))?;
    let system_time = metadata
        .created()
        .or_else(|_| metadata.modified())
        .with_context(|| format!("Geen tijdstempel beschikbaar voor {}", path.display()))?;
    let datetime: DateTime<Local> = system_time.into();
    let date = datetime.format("%Y-%m-%d").to_string();
    let time = datetime.format("%H:%M:%S").to_string();
    Ok((date, time))
}

/// Parses a comma separated latitude and longitude tuple.
///
/// # Errors
///
/// Returns an error when the input cannot be split into two valid floating
/// point numbers.
pub fn parse_coordinates(input: &str) -> anyhow::Result<(f64, f64)> {
    let trimmed = input.trim();
    let (lat_str, lng_str) = trimmed
        .split_once(',')
        .ok_or_else(|| anyhow!("Gebruik het formaat '<lat>, <lng>'."))?;
    let lat = lat_str
        .trim()
        .parse::<f64>()
        .with_context(|| "Latitude kon niet gelezen worden")?;
    let lng = lng_str
        .trim()
        .parse::<f64>()
        .with_context(|| "Longitude kon niet gelezen worden")?;
    Ok((lat, lng))
}

/// Loads the Feedie application icon that is displayed in the platform window.
///
/// If the embedded PNG cannot be decoded, this logs a warning and returns the
/// default egui icon data.
pub fn load_app_icon() -> IconData {
    const ICON_BYTES: &[u8] = include_bytes!("../../../assets/Feedie_icon.png");
    match image::load_from_memory(ICON_BYTES) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            IconData {
                rgba: rgba.into_raw(),
                width,
                height,
            }
        }
        Err(err) => {
            tracing::warn!("Kon app-icoon niet laden: {err}");
            IconData::default()
        }
    }
}
