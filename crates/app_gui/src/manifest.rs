//! Handling of remote manifests and model downloads.

use crate::app::{LABEL_FILE_NAME, MANIFEST_URL, MODEL_FILE_NAME, UiApp, VERSION_FILE_NAME};
use crate::model::{normalize_model_version, read_model_version_from};
use anyhow::{Context, anyhow};
use eframe::egui;
use hex::encode as hex_encode;
use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;
use zip::ZipArchive;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use std::process::Command;
#[cfg(target_os = "windows")]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Summary of available updates for the application and model.
///
/// This mirrors the user-facing information in the “Versies” section and is
/// populated by [`apply_manifest`].
#[derive(Clone, Default)]
pub(crate) struct UpdateSummary {
    pub(crate) latest_app: String,
    pub(crate) app_url: String,
    pub(crate) app_windows_url: Option<String>,
    pub(crate) app_windows_sha256: Option<String>,
    pub(crate) app_windows_size_mb: Option<f32>,
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
    Error(ManifestError),
}

/// Manifest check errors that can be localized in the UI.
#[derive(Clone, Copy)]
pub(crate) enum ManifestError {
    CheckFailed,
}

impl ManifestError {
    fn message_key(self) -> &'static str {
        match self {
            ManifestError::CheckFailed => "updates-check-failed",
        }
    }
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

/// Status for background app update downloads.
#[derive(Clone, Default)]
pub(crate) enum AppDownloadStatus {
    #[default]
    Idle,
    Downloading,
    Installing,
    Error(AppDownloadError),
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Clone, Copy)]
pub(crate) enum AppDownloadError {
    DownloadFailed,
    HashMismatch,
    UpdaterMissing,
    InstallFailed,
}

impl AppDownloadError {
    fn message_key(self) -> &'static str {
        match self {
            AppDownloadError::DownloadFailed => "updates-app-download-failed",
            AppDownloadError::HashMismatch => "updates-app-hash-mismatch",
            AppDownloadError::UpdaterMissing => "updates-app-updater-missing",
            AppDownloadError::InstallFailed => "updates-app-install-failed",
        }
    }
}

impl UiApp {
    /// Starts a background task that fetches the remote manifest file.
    ///
    /// The call is idempotent while a previous fetch is still running.
    pub(crate) fn request_manifest_refresh(&mut self) {
        if matches!(self.manifest_status, ManifestStatus::Checking) {
            return;
        }
        let (tx, rx) = mpsc::channel::<Result<RemoteManifest, ManifestError>>();
        self.update_rx = Some(rx);
        self.manifest_status = ManifestStatus::Checking;
        thread::spawn(move || {
            let result = fetch_remote_manifest().map_err(|err| {
                tracing::warn!("Manifest fetch failed: {err}");
                ManifestError::CheckFailed
            });
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
                    self.manifest_status = ManifestStatus::Error(ManifestError::CheckFailed);
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
        let (app_windows_url, app_windows_sha256, app_windows_size_mb) =
            match manifest.app.windows.as_ref() {
                Some(asset) => (Some(asset.url.clone()), asset.sha256.clone(), asset.size_mb),
                None => (None, None, None),
            };
        let mut summary = UpdateSummary {
            latest_app: latest_app.clone(),
            app_url: manifest.app.url.clone(),
            app_windows_url,
            app_windows_sha256,
            app_windows_size_mb,
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
        ui.heading(self.t("updates-title"));
        match &self.manifest_status {
            ManifestStatus::Idle => {
                if ui.button(self.t("updates-check")).clicked() {
                    self.request_manifest_refresh();
                }
            }
            ManifestStatus::Checking => {
                ui.label(self.t("updates-checking"));
            }
            ManifestStatus::Error(err) => {
                ui.colored_label(egui::Color32::RED, self.t(err.message_key()));
                if ui.button(self.t("action-try-again")).clicked() {
                    self.request_manifest_refresh();
                }
            }
            ManifestStatus::Ready(summary) => {
                let summary = summary.clone();
                if summary.app_update_available {
                    ui.label(format!(
                        "{}: {}",
                        self.t("updates-app-available"),
                        summary.latest_app
                    ));
                    if let Some(size) = summary.app_windows_size_mb.filter(|size| *size > 0.0) {
                        ui.label(format!("{}: {:.1} MB", self.t("updates-app-size"), size));
                    }
                    if cfg!(target_os = "windows") && summary.app_windows_url.is_some() {
                        self.render_app_download_actions(ui, &summary);
                    } else {
                        ui.hyperlink_to(self.t("updates-open-download"), &summary.app_url);
                    }
                } else {
                    ui.label(self.t("updates-app-latest"));
                }
                ui.add_space(4.0);
                if summary.model_update_available {
                    ui.label(format!(
                        "{}: {}",
                        self.t("updates-model-available"),
                        summary.latest_model
                    ));
                    if let Some(size) = summary.model_size_mb {
                        ui.label(format!("{}: {:.1} MB", self.t("updates-model-size"), size));
                    }
                    if let Some(notes) = &summary.model_notes {
                        ui.label(notes);
                    }
                    self.render_model_download_actions(ui, &summary);
                } else {
                    ui.label(self.t("updates-model-latest"));
                    self.render_model_download_feedback(ui);
                }
                ui.add_space(6.0);
                if ui.button(self.t("updates-check-again")).clicked() {
                    self.request_manifest_refresh();
                }
            }
        }
    }

    /// Shows the call-to-action buttons for downloading the new app version.
    pub(crate) fn render_app_download_actions(
        &mut self,
        ui: &mut egui::Ui,
        summary: &UpdateSummary,
    ) {
        match &self.app_download_status {
            AppDownloadStatus::Idle => {
                if ui.button(self.t("updates-app-download-install")).clicked() {
                    self.start_app_download(summary);
                }
            }
            AppDownloadStatus::Downloading => {
                ui.label(self.t("updates-app-downloading"));
            }
            AppDownloadStatus::Installing => {
                ui.label(self.t("updates-app-installing"));
            }
            AppDownloadStatus::Error(err) => {
                ui.colored_label(egui::Color32::RED, self.t(err.message_key()));
                if ui.button(self.t("updates-download-again")).clicked() {
                    self.start_app_download(summary);
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
                if ui.button(self.t("updates-download-install")).clicked() {
                    self.start_model_download(summary);
                }
            }
            ModelDownloadStatus::Downloading => {
                ui.label(self.t("updates-model-downloading"));
            }
            ModelDownloadStatus::Error(err) => {
                ui.colored_label(egui::Color32::RED, err);
                if ui.button(self.t("updates-download-again")).clicked() {
                    self.start_model_download(summary);
                }
            }
            ModelDownloadStatus::Success(msg) => {
                ui.label(msg);
                if ui.button(self.t("updates-download-again")).clicked() {
                    self.start_model_download(summary);
                }
            }
        }
    }

    /// Displays feedback about the last download attempt when no update is available.
    pub(crate) fn render_model_download_feedback(&self, ui: &mut egui::Ui) {
        match &self.model_download_status {
            ModelDownloadStatus::Idle => {
                ui.label(self.t("updates-no-downloads"));
            }
            ModelDownloadStatus::Downloading => {
                ui.label(self.t("updates-download-progress"));
            }
            ModelDownloadStatus::Error(err) => {
                ui.colored_label(egui::Color32::RED, err);
            }
            ModelDownloadStatus::Success(msg) => {
                ui.label(msg);
            }
        }
    }

    /// Initiates the download of a new app installer in the background.
    pub(crate) fn start_app_download(&mut self, summary: &UpdateSummary) {
        if matches!(
            self.app_download_status,
            AppDownloadStatus::Downloading | AppDownloadStatus::Installing
        ) {
            return;
        }
        let Some(url) = summary.app_windows_url.clone() else {
            return;
        };
        let expected_hash = summary.app_windows_sha256.clone();
        let version = summary.latest_app.clone();
        let (tx, rx) = mpsc::channel();
        self.app_download_rx = Some(rx);
        self.app_download_status = AppDownloadStatus::Downloading;
        thread::spawn(move || {
            let result = download_app_installer(&url, expected_hash.as_deref(), &version);
            let _ = tx.send(result);
        });
    }

    /// Polls the app update download task and triggers the installer.
    pub(crate) fn poll_app_download(&mut self) {
        if let Some(rx) = self.app_download_rx.take() {
            match rx.try_recv() {
                Ok(Ok(installer_path)) => {
                    self.app_download_status = AppDownloadStatus::Installing;
                    if let Err(err) = self.launch_app_update(&installer_path) {
                        self.app_download_status = AppDownloadStatus::Error(err);
                    }
                }
                Ok(Err(err)) => {
                    self.app_download_status = AppDownloadStatus::Error(err);
                }
                Err(TryRecvError::Empty) => {
                    self.app_download_rx = Some(rx);
                }
                Err(TryRecvError::Disconnected) => {
                    self.app_download_status =
                        AppDownloadStatus::Error(AppDownloadError::DownloadFailed);
                }
            }
        }
    }

    fn launch_app_update(&self, installer_path: &Path) -> Result<(), AppDownloadError> {
        #[cfg(target_os = "windows")]
        {
            let app_exe = env::current_exe().map_err(|err| {
                tracing::warn!("Kon app-pad niet bepalen: {err}");
                AppDownloadError::InstallFailed
            })?;
            let updater_source = app_exe
                .parent()
                .ok_or(AppDownloadError::InstallFailed)?
                .join("FeedieUpdater.exe");
            if !updater_source.exists() {
                tracing::warn!("FeedieUpdater.exe ontbreekt naast {}", app_exe.display());
                return Err(AppDownloadError::UpdaterMissing);
            }

            let updater_dir = installer_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(env::temp_dir);
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let updater_path = updater_dir.join(format!("FeedieUpdater-{stamp}.exe"));
            fs::copy(&updater_source, &updater_path).map_err(|err| {
                tracing::warn!(
                    "Kon updater niet kopieren van {} naar {}: {err}",
                    updater_source.display(),
                    updater_path.display()
                );
                AppDownloadError::InstallFailed
            })?;

            let log_path = installer_path.with_extension("log");
            let mut cmd = Command::new(&updater_path);
            cmd.arg("--installer")
                .arg(installer_path)
                .arg("--app")
                .arg(app_exe)
                .arg("--log")
                .arg(log_path)
                .arg("--cleanup")
                .creation_flags(CREATE_NO_WINDOW);
            cmd.spawn().map_err(|err| {
                tracing::warn!("Kon updater {} niet starten: {err}", updater_path.display());
                AppDownloadError::InstallFailed
            })?;
            std::process::exit(0);
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = installer_path;
            Err(AppDownloadError::InstallFailed)
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
                        "{} {normalized}.",
                        self.t("updates-model-installed")
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
                        ModelDownloadStatus::Error(self.t("updates-download-channel-closed"));
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
    #[serde(default)]
    windows: Option<PlatformAsset>,
}

#[derive(Debug, Deserialize)]
struct PlatformAsset {
    url: String,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    size_mb: Option<f32>,
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
        .get(manifest_url())
        .send()
        .context("Manifest kon niet worden opgehaald")?
        .error_for_status()
        .context("Manifest gaf een foutstatus terug")?;
    let manifest = response
        .json::<RemoteManifest>()
        .context("Manifest kon niet worden geparseerd")?;
    Ok(manifest)
}

fn manifest_url() -> String {
    match env::var("FEEDIE_MANIFEST_URL") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => MANIFEST_URL.to_string(),
    }
}

/// Returns true if `latest` represents a version newer than `current`.
fn version_is_newer(latest: &str, current: &str) -> bool {
    match (Version::parse(latest), Version::parse(current)) {
        (Ok(lat), Ok(curr)) => lat > curr,
        _ => latest != current,
    }
}

/// Downloads the app installer from `url` into a temporary location.
fn download_app_installer(
    url: &str,
    expected_sha256: Option<&str>,
    version: &str,
) -> Result<PathBuf, AppDownloadError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|err| {
            tracing::warn!("HTTP-client kon niet worden opgebouwd: {err}");
            AppDownloadError::DownloadFailed
        })?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|err| {
            tracing::warn!("App-update kon niet worden opgehaald: {err}");
            AppDownloadError::DownloadFailed
        })?
        .error_for_status()
        .map_err(|err| {
            tracing::warn!("Server gaf een foutstatus terug: {err}");
            AppDownloadError::DownloadFailed
        })?;

    let file_name = file_name_from_url(url).unwrap_or_else(|| format!("FeedieSetup-{version}.exe"));
    let target_dir = env::temp_dir().join("FeedieUpdate").join(version);
    fs::create_dir_all(&target_dir).map_err(|err| {
        tracing::warn!("Kon tijdelijke update-map niet maken: {err}");
        AppDownloadError::DownloadFailed
    })?;
    let installer_path = target_dir.join(file_name);
    {
        let mut file = fs::File::create(&installer_path).map_err(|err| {
            tracing::warn!("Kon tijdelijk downloadbestand niet openen: {err}");
            AppDownloadError::DownloadFailed
        })?;
        io::copy(&mut response, &mut file).map_err(|err| {
            tracing::warn!("Download kon niet worden opgeslagen: {err}");
            AppDownloadError::DownloadFailed
        })?;
    }

    let expected_sha256 = expected_sha256.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    if let Some(expected) = expected_sha256 {
        let actual = sha256_file(&installer_path).map_err(|err| {
            tracing::warn!("Kon SHA256 niet berekenen: {err}");
            AppDownloadError::DownloadFailed
        })?;
        if !actual.eq_ignore_ascii_case(expected) {
            tracing::warn!("SHA256 mismatch: expected {expected}, actual {actual}");
            return Err(AppDownloadError::HashMismatch);
        }
    }

    Ok(installer_path)
}

fn file_name_from_url(url: &str) -> Option<String> {
    url.split('/')
        .next_back()
        .map(|name| name.split('?').next().unwrap_or(name))
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = fs::File::open(path).context("Kon bestand niet openen voor hash")?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).context("Kon bestand niet hashen")?;
    let digest = hasher.finalize();
    Ok(hex_encode(digest))
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
