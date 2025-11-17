//! Handling of remote manifests and model downloads.

use crate::app::{LABEL_FILE_NAME, MANIFEST_URL, MODEL_FILE_NAME, UiApp, VERSION_FILE_NAME};
use crate::model::{normalize_model_version, read_model_version_from};
use anyhow::{Context, anyhow};
use eframe::egui;
use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;
use zip::ZipArchive;

/// Summary of available updates for the application and model.
///
/// This mirrors the user-facing information in the “Versies” section and is
/// populated by [`apply_manifest`].
#[derive(Clone, Default)]
pub(crate) struct UpdateSummary {
    pub(crate) latest_app: String,
    pub(crate) app_url: String,
    pub(crate) latest_model: String,
    pub(crate) model_url: String,
    pub(crate) app_update_available: bool,
    pub(crate) model_update_available: bool,
    pub(crate) model_size_mb: Option<f32>,
    pub(crate) model_notes: Option<String>,
}

/// Status for fetching the remote manifest.
#[derive(Clone, Default)]
pub(crate) enum ManifestStatus {
    #[default]
    Idle,
    Checking,
    Ready(UpdateSummary),
    Error(String),
}

/// Status for background model downloads.
#[derive(Clone, Default)]
pub(crate) enum ModelDownloadStatus {
    #[default]
    Idle,
    Downloading,
    Success(String),
    Error(String),
}

impl UiApp {
    /// Starts a background task that fetches the remote manifest file.
    ///
    /// The call is idempotent while a previous fetch is still running.
    pub(crate) fn request_manifest_refresh(&mut self) {
        if matches!(self.manifest_status, ManifestStatus::Checking) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.update_rx = Some(rx);
        self.manifest_status = ManifestStatus::Checking;
        thread::spawn(move || {
            let result = fetch_remote_manifest().map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }

    /// Polls the manifest channel for results.
    pub(crate) fn poll_manifest_updates(&mut self) {
        if let Some(rx) = self.update_rx.take() {
            match rx.try_recv() {
                Ok(Ok(manifest)) => self.apply_manifest(manifest),
                Ok(Err(err)) => self.manifest_status = ManifestStatus::Error(err),
                Err(TryRecvError::Empty) => self.update_rx = Some(rx),
                Err(TryRecvError::Disconnected) => {
                    self.manifest_status =
                        ManifestStatus::Error("Zoeken naar updates is mislukt".to_string());
                }
            }
        }
    }

    /// Applies the newly fetched manifest to the UI state and stamps the change.
    pub(crate) fn apply_manifest(&mut self, manifest: RemoteManifest) {
        let latest_app = manifest.app.latest.clone();
        let latest_model = manifest.model.latest.clone();
        let normalized_latest_model = normalize_model_version(&latest_model);
        let normalized_current_model = normalize_model_version(&self.model_version);
        let mut summary = UpdateSummary {
            latest_app: latest_app.clone(),
            app_url: manifest.app.url.clone(),
            latest_model: latest_model.clone(),
            model_url: manifest.model.url.clone(),
            app_update_available: version_is_newer(&latest_app, &self.app_version),
            model_update_available: version_is_newer(
                &normalized_latest_model,
                &normalized_current_model,
            ),
            model_size_mb: manifest.model.size_mb,
            model_notes: manifest.model.notes.clone(),
        };
        if !summary.app_update_available && !summary.model_update_available {
            summary.model_notes = manifest.model.notes;
        }
        self.manifest_status = ManifestStatus::Ready(summary);
    }

    /// Renders the update information inside the settings panel.
    ///
    /// This helper pulls state from [`ManifestStatus`] and exposes the
    /// “Controleer op updates” affordances to the user.
    pub(crate) fn render_update_section(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);
        ui.heading("Updates");
        match &self.manifest_status {
            ManifestStatus::Idle => {
                if ui.button("Controleer op updates").clicked() {
                    self.request_manifest_refresh();
                }
            }
            ManifestStatus::Checking => {
                ui.label("Zoeken naar updates...");
            }
            ManifestStatus::Error(err) => {
                ui.colored_label(egui::Color32::RED, err);
                if ui.button("Opnieuw proberen").clicked() {
                    self.request_manifest_refresh();
                }
            }
            ManifestStatus::Ready(summary) => {
                let summary = summary.clone();
                if summary.app_update_available {
                    ui.label(format!(
                        "Nieuwe app-versie beschikbaar: {}",
                        summary.latest_app
                    ));
                    ui.hyperlink_to("Open downloadpagina", &summary.app_url);
                } else {
                    ui.label("Je gebruikt de nieuwste app-versie.");
                }
                ui.add_space(4.0);
                if summary.model_update_available {
                    ui.label(format!(
                        "Nieuw herkenningsmodel beschikbaar: {}",
                        summary.latest_model
                    ));
                    if let Some(size) = summary.model_size_mb {
                        ui.label(format!("Geschatte downloadgrootte: {:.1} MB", size));
                    }
                    if let Some(notes) = &summary.model_notes {
                        ui.label(notes);
                    }
                    self.render_model_download_actions(ui, &summary);
                } else {
                    ui.label("Herkenningsmodel is up-to-date.");
                    self.render_model_download_feedback(ui);
                }
                ui.add_space(6.0);
                if ui.button("Opnieuw controleren").clicked() {
                    self.request_manifest_refresh();
                }
            }
        }
    }

    /// Shows the call-to-action buttons for downloading the new model.
    pub(crate) fn render_model_download_actions(
        &mut self,
        ui: &mut egui::Ui,
        summary: &UpdateSummary,
    ) {
        match &self.model_download_status {
            ModelDownloadStatus::Idle => {
                if ui.button("Download en installeren").clicked() {
                    self.start_model_download(summary);
                }
            }
            ModelDownloadStatus::Downloading => {
                ui.label("Modelupdate wordt gedownload...");
            }
            ModelDownloadStatus::Error(err) => {
                ui.colored_label(egui::Color32::RED, err);
                if ui.button("Opnieuw downloaden").clicked() {
                    self.start_model_download(summary);
                }
            }
            ModelDownloadStatus::Success(msg) => {
                ui.label(msg);
                if ui.button("Download opnieuw").clicked() {
                    self.start_model_download(summary);
                }
            }
        }
    }

    /// Displays feedback about the last download attempt when no update is available.
    pub(crate) fn render_model_download_feedback(&self, ui: &mut egui::Ui) {
        match &self.model_download_status {
            ModelDownloadStatus::Idle => {
                ui.label("Geen recente modeldownloads uitgevoerd.");
            }
            ModelDownloadStatus::Downloading => {
                ui.label("Modeldownload wordt uitgevoerd...");
            }
            ModelDownloadStatus::Error(err) => {
                ui.colored_label(egui::Color32::RED, err);
            }
            ModelDownloadStatus::Success(msg) => {
                ui.label(msg);
            }
        }
    }

    /// Initiates the download of a new model in the background.
    pub(crate) fn start_model_download(&mut self, summary: &UpdateSummary) {
        if matches!(self.model_download_status, ModelDownloadStatus::Downloading) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.model_download_rx = Some(rx);
        self.model_download_status = ModelDownloadStatus::Downloading;
        let url = summary.model_url.clone();
        let target_root = self.model_root.clone();
        let version = summary.latest_model.clone();
        thread::spawn(move || {
            let result = download_and_install_model(&url, &target_root, &version)
                .map(|_| version.clone())
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }

    /// Polls the download task and updates the UI with the result.
    pub(crate) fn poll_model_download(&mut self) {
        if let Some(rx) = self.model_download_rx.take() {
            match rx.try_recv() {
                Ok(Ok(version)) => {
                    let normalized = normalize_model_version(&version);
                    self.model_download_status = ModelDownloadStatus::Success(format!(
                        "Model {normalized} ge\u{EB}nstalleerd."
                    ));
                    self.model_version = read_model_version_from(&self.model_version_path());
                    self.label_options = Self::load_label_options_from(&self.labels_path());
                    self.request_manifest_refresh();
                }
                Ok(Err(err)) => {
                    self.model_download_status = ModelDownloadStatus::Error(err);
                }
                Err(TryRecvError::Empty) => {
                    self.model_download_rx = Some(rx);
                }
                Err(TryRecvError::Disconnected) => {
                    self.model_download_status =
                        ModelDownloadStatus::Error("Downloadkanaal verbroken".to_string());
                }
            }
        }
    }
}

/// JSON layout returned by the remote manifest endpoint.
#[derive(Debug, Deserialize)]
pub(crate) struct RemoteManifest {
    app: ManifestEntry,
    model: ModelManifestEntry,
}

/// Manifest subsection describing the application binary.
#[derive(Debug, Deserialize)]
struct ManifestEntry {
    latest: String,
    url: String,
}

/// Manifest subsection describing the downloadable recognition model.
#[derive(Debug, Deserialize)]
struct ModelManifestEntry {
    latest: String,
    url: String,
    #[serde(default)]
    _labels_hash: Option<String>,
    #[serde(default)]
    size_mb: Option<f32>,
    #[serde(default)]
    notes: Option<String>,
}

/// Downloads and parses the JSON manifest that describes available updates.
fn fetch_remote_manifest() -> anyhow::Result<RemoteManifest> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("HTTP-client kon niet worden opgebouwd")?;
    let response = client
        .get(MANIFEST_URL)
        .send()
        .context("Manifest kon niet worden opgehaald")?
        .error_for_status()
        .context("Manifest gaf een foutstatus terug")?;
    let manifest = response
        .json::<RemoteManifest>()
        .context("Manifest kon niet worden geparseerd")?;
    Ok(manifest)
}

/// Returns true if `latest` represents a version newer than `current`.
fn version_is_newer(latest: &str, current: &str) -> bool {
    match (Version::parse(latest), Version::parse(current)) {
        (Ok(lat), Ok(curr)) => lat > curr,
        _ => latest != current,
    }
}

/// Downloads the model ZIP from `url` and installs it into `target_root`.
fn download_and_install_model(url: &str, target_root: &Path, version: &str) -> anyhow::Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("HTTP-client kon niet worden opgebouwd")?;
    let mut response = client
        .get(url)
        .send()
        .context("Modelupdate kon niet worden opgehaald")?
        .error_for_status()
        .context("Server gaf een foutstatus terug")?;
    let temp_dir = tempdir().context("Kon tijdelijke map niet aanmaken")?;
    let archive_path = temp_dir.path().join("model_update.zip");
    {
        let mut file =
            fs::File::create(&archive_path).context("Kon tijdelijk downloadbestand niet openen")?;
        io::copy(&mut response, &mut file).context("Download kon niet worden opgeslagen")?;
    }
    let extract_dir = temp_dir.path().join("extracted");
    fs::create_dir_all(&extract_dir).context("Kon tijdelijke uitpakmap niet aanmaken")?;
    let reader = fs::File::open(&archive_path).context("Kon gedownload bestand niet openen")?;
    let mut archive = ZipArchive::new(reader).context("Modelupdate is geen geldig ZIP-bestand")?;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .context("ZIP-bestand kon niet worden gelezen")?;
        let outpath = extract_dir.join(file.mangled_name());
        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }
    fs::create_dir_all(target_root).context("Kon doelmap voor model niet aanmaken")?;
    for name in [MODEL_FILE_NAME, LABEL_FILE_NAME] {
        let src = extract_dir.join(name);
        if !src.exists() {
            return Err(anyhow!("Bestand {name} ontbreekt in modelupdate."));
        }
        let dest = target_root.join(name);
        fs::copy(&src, &dest).with_context(|| {
            format!(
                "Kopi\u{EB}ren van {} naar {} mislukt",
                src.display(),
                dest.display()
            )
        })?;
    }
    let version_src = extract_dir.join(VERSION_FILE_NAME);
    if version_src.exists() {
        let dest = target_root.join(VERSION_FILE_NAME);
        fs::copy(&version_src, &dest).with_context(|| {
            format!(
                "Kon modelversie niet bijwerken vanuit {}",
                version_src.display()
            )
        })?;
    } else {
        fs::write(target_root.join(VERSION_FILE_NAME), version)
            .context("Kon modelversie niet opslaan")?;
    }
    Ok(())
}
