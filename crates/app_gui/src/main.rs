#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use anyhow::{Context, anyhow};
use arboard::Clipboard;
use chrono::{DateTime, Local};
use directories_next::ProjectDirs;
use eframe::{App, Frame, NativeOptions, egui};
use egui::viewport::{IconData, ViewportBuilder};
use feeder_core::{
    Classification, ClassifierConfig, Decision, EfficientVitClassifier, ImageInfo, ScanOptions,
    scan_folder_with,
};
use rfd::FileDialog;
use semver::Version;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;
use zip::ZipArchive;

fn main() {
    #[cfg(debug_assertions)]
    tracing_subscriber::fmt::init();
    let options = NativeOptions {
        viewport: ViewportBuilder::default().with_icon(Arc::new(load_app_icon())),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "Feedie",
        options,
        Box::new(|_cc| {
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(Box::new(UiApp::default()))
        }),
    ) {
        eprintln!("Applicatie gestopt met fout: {e}");
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    #[default]
    Aanwezig,
    Leeg,
    Onzeker,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Folder,
    Results,
    Export,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreviewAction {
    None,
    Prev,
    Next,
    Close,
}

#[derive(Clone)]
struct PreviewState {
    view: ViewMode,
    current: usize,
    open: bool,
    viewport_id: egui::ViewportId,
    initialized: bool,
}

#[derive(Clone)]
struct LabelOption {
    canonical: String,
    display: String,
    scientific: Option<String>,
}

#[derive(Clone)]
struct ExportOptions {
    include_present: bool,
    include_uncertain: bool,
    include_background: bool,
    include_csv: bool,
}

struct PendingExport {
    target_dir: PathBuf,
    options: ExportOptions,
}

struct ExportOutcome {
    copied: usize,
    wrote_csv: bool,
    target_dir: PathBuf,
}

struct ExportJob {
    source: PathBuf,
    folder_label: String,
    canonical_label: Option<String>,
    include_in_csv: bool,
}

struct CsvRecord {
    date: String,
    time: String,
    scientific: String,
    path: String,
}

#[derive(Default)]
struct CoordinatePrompt {
    input: String,
    error: Option<String>,
}

#[derive(Clone, Default)]
struct UpdateSummary {
    latest_app: String,
    app_url: String,
    latest_model: String,
    model_url: String,
    app_update_available: bool,
    model_update_available: bool,
    model_size_mb: Option<f32>,
    model_notes: Option<String>,
}

#[derive(Clone, Default)]
enum ManifestStatus {
    #[default]
    Idle,
    Checking,
    Ready(UpdateSummary),
    Error(String),
}

#[derive(Clone, Default)]
enum ModelDownloadStatus {
    #[default]
    Idle,
    Downloading,
    Success(String),
    Error(String),
}

struct UiApp {
    gekozen_map: Option<PathBuf>,
    rijen: Vec<ImageInfo>,
    total_files: usize,
    scanned_count: usize,
    has_scanned: bool,
    scan_in_progress: bool,
    status: String,
    view: ViewMode,
    panel: Panel,
    rx: Option<Receiver<ScanMsg>>,
    thumbs: HashMap<PathBuf, egui::TextureHandle>,
    thumb_keys: VecDeque<PathBuf>,
    full_images: HashMap<PathBuf, egui::TextureHandle>,
    full_keys: VecDeque<PathBuf>,
    selected_indices: BTreeSet<usize>,
    selection_anchor: Option<usize>,
    presence_threshold: f32,
    pending_presence_threshold: f32,
    batch_size: usize,
    background_labels_input: String,
    background_labels: Vec<String>,
    preview: Option<PreviewState>,
    label_options: Vec<LabelOption>,
    new_label_buffer: String,
    export_present: bool,
    export_uncertain: bool,
    export_background: bool,
    export_csv: bool,
    pending_export: Option<PendingExport>,
    coordinate_prompt: Option<CoordinatePrompt>,
    manifest_status: ManifestStatus,
    update_rx: Option<Receiver<Result<RemoteManifest, String>>>,
    model_download_status: ModelDownloadStatus,
    model_download_rx: Option<Receiver<Result<String, String>>>,
    app_version: String,
    model_version: String,
    model_root: PathBuf,
    // Settings: Roboflow
    improve_recognition: bool,
    roboflow_dataset_input: String,
    upload_status_tx: Sender<String>,
    upload_status_rx: Receiver<String>,
}

impl UiApp {
    fn new() -> Self {
        let mut app = Self::default_internal();
        app.request_manifest_refresh();
        app
    }

    fn default_internal() -> Self {
        let (model_root, model_version) = Self::prepare_model_dir();
        let label_options = Self::load_label_options_from(&model_root.join("feeder-labels.csv"));
        let (upload_status_tx, upload_status_rx) = mpsc::channel();
        Self {
            gekozen_map: None,
            rijen: Vec::new(),
            total_files: 0,
            scanned_count: 0,
            has_scanned: false,
            scan_in_progress: false,
            status: String::new(),
            view: ViewMode::default(),
            panel: Panel::Folder,
            rx: None,
            thumbs: HashMap::new(),
            thumb_keys: VecDeque::new(),
            full_images: HashMap::new(),
            full_keys: VecDeque::new(),
            selected_indices: BTreeSet::new(),
            selection_anchor: None,
            presence_threshold: 0.5,
            pending_presence_threshold: 0.5,
            batch_size: 8,
            background_labels_input: "Achtergrond".to_string(),
            background_labels: vec!["achtergrond".to_string()],
            preview: None,
            label_options,
            new_label_buffer: String::new(),
            export_present: true,
            export_uncertain: false,
            export_background: false,
            export_csv: true,
            pending_export: None,
            coordinate_prompt: None,
            manifest_status: ManifestStatus::Idle,
            update_rx: None,
            model_download_status: ModelDownloadStatus::Idle,
            model_download_rx: None,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            model_version,
            model_root,
            improve_recognition: false,
            roboflow_dataset_input: "voederhuiscamera".to_string(),
            upload_status_tx,
            upload_status_rx,
        }
    }
}

impl Default for UiApp {
    fn default() -> Self {
        Self::new()
    }
}

const THUMB_SIZE: u32 = 120;
const MAX_THUMBS: usize = 256;
const MAX_FULL_IMAGES: usize = 32;
const MAX_THUMB_LOAD_PER_FRAME: usize = 12;
const CARD_WIDTH: f32 = THUMB_SIZE as f32 + 40.0;
const CARD_HEIGHT: f32 = THUMB_SIZE as f32 + 70.0;
const ROBOFLOW_API_KEY: &str = "g9zfZxZVNuSr43ENZJMg";
const MANIFEST_URL: &str = "https://github.com/kpauly/feeder-vision/raw/main/manifest.json";
const MODEL_FILE_NAME: &str = "feeder-efficientvit-m0.safetensors";
const LABEL_FILE_NAME: &str = "feeder-labels.csv";
const VERSION_FILE_NAME: &str = "model_version.txt";

enum ScanMsg {
    Progress(usize, usize),     // scanned, total
    Done(Vec<ImageInfo>, u128), // rows, elapsed_ms
    Error(String),
}

impl UiApp {
    fn get_or_load_thumb(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureId> {
        if let Some(tex) = self.thumbs.get(path) {
            return Some(tex.id());
        }

        match image::open(path) {
            Ok(img) => {
                // Ensure a 4-channel buffer for egui
                let rgba = img.to_rgba8();
                let thumb = image::imageops::thumbnail(&rgba, THUMB_SIZE, THUMB_SIZE);
                let (w, h) = thumb.dimensions();
                let size = [w as usize, h as usize];
                let pixels = thumb.into_raw(); // RGBA, len = w*h*4
                let color = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                let name = format!("thumb:{}", path.display());
                let tex = ctx.load_texture(name, color, egui::TextureOptions::LINEAR);
                self.thumbs.insert(path.to_path_buf(), tex);
                self.thumb_keys.push_back(path.to_path_buf());
                if self.thumbs.len() > MAX_THUMBS
                    && let Some(old) = self.thumb_keys.pop_front()
                {
                    self.thumbs.remove(&old);
                }
                self.thumbs.get(path).map(|t| t.id())
            }
            Err(e) => {
                tracing::warn!("Failed to load thumbnail for {}: {}", path.display(), e);
                None
            }
        }
    }

    fn get_or_load_full_image(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
    ) -> Option<egui::TextureHandle> {
        if let Some(tex) = self.full_images.get(path) {
            return Some(tex.clone());
        }
        match image::open(path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let size = [w as usize, h as usize];
                let pixels = rgba.into_raw();
                let color = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                let name = format!("full:{}", path.display());
                let tex = ctx.load_texture(name, color, egui::TextureOptions::LINEAR);
                self.full_images.insert(path.to_path_buf(), tex.clone());
                self.full_keys.push_back(path.to_path_buf());
                while self.full_images.len() > MAX_FULL_IMAGES {
                    if let Some(old) = self.full_keys.pop_front() {
                        self.full_images.remove(&old);
                    } else {
                        break;
                    }
                }
                Some(tex)
            }
            Err(e) => {
                tracing::warn!("Failed to load full image for {}: {}", path.display(), e);
                None
            }
        }
    }

    fn indices_for_view(&self, view: ViewMode) -> Vec<usize> {
        self.rijen
            .iter()
            .enumerate()
            .filter_map(|(idx, info)| match view {
                ViewMode::Aanwezig if info.present && !self.is_onzeker(info) => Some(idx),
                ViewMode::Leeg if !info.present && !self.is_onzeker(info) => Some(idx),
                ViewMode::Onzeker if self.is_onzeker(info) => Some(idx),
                _ => None,
            })
            .collect()
    }

    fn filtered_indices(&self) -> Vec<usize> {
        self.indices_for_view(self.view)
    }

    fn view_counts(&self) -> (usize, usize, usize) {
        let mut present = 0usize;
        let mut empty = 0usize;
        let mut unsure = 0usize;
        for info in &self.rijen {
            if self.is_onzeker(info) {
                unsure += 1;
            } else if info.present {
                present += 1;
            } else {
                empty += 1;
            }
        }
        (present, empty, unsure)
    }

    fn handle_select_shortcuts(&mut self, ctx: &egui::Context, filtered: &[usize]) {
        let mut trigger_select_all = false;
        ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::A) {
                trigger_select_all = true;
            }
        });
        if trigger_select_all {
            self.select_all(filtered);
        }
    }

    fn select_single(&mut self, idx: usize) {
        self.selected_indices.clear();
        self.selected_indices.insert(idx);
        self.selection_anchor = Some(idx);
    }

    fn toggle_selection(&mut self, idx: usize) {
        if self.selected_indices.contains(&idx) {
            self.selected_indices.remove(&idx);
        } else {
            self.selected_indices.insert(idx);
            self.selection_anchor = Some(idx);
        }
    }

    fn select_range_in_view(&mut self, filtered: &[usize], target_idx: usize) {
        let Some(anchor_idx) = self.selection_anchor else {
            self.select_single(target_idx);
            return;
        };
        let Some(anchor_pos) = filtered.iter().position(|&v| v == anchor_idx) else {
            self.select_single(target_idx);
            return;
        };
        let Some(target_pos) = filtered.iter().position(|&v| v == target_idx) else {
            self.select_single(target_idx);
            return;
        };
        let (start, end) = if anchor_pos <= target_pos {
            (anchor_pos, target_pos)
        } else {
            (target_pos, anchor_pos)
        };
        self.selected_indices.clear();
        for &idx in &filtered[start..=end] {
            self.selected_indices.insert(idx);
        }
        self.selection_anchor = Some(target_idx);
    }

    fn select_all(&mut self, filtered: &[usize]) {
        self.selected_indices.clear();
        for &idx in filtered {
            self.selected_indices.insert(idx);
        }
        self.selection_anchor = filtered.first().copied();
    }

    fn handle_selection_click(
        &mut self,
        filtered: &[usize],
        idx: usize,
        modifiers: egui::Modifiers,
    ) {
        if modifiers.shift {
            self.select_range_in_view(filtered, idx);
        } else if modifiers.command {
            self.toggle_selection(idx);
        } else {
            self.select_single(idx);
        }
    }

    fn open_preview(&mut self, filtered: &[usize], idx: usize) {
        if let Some(pos) = filtered.iter().position(|&i| i == idx) {
            let viewport_id =
                egui::ViewportId::from_hash_of(("preview", self.view as u8, filtered[pos]));
            self.preview = Some(PreviewState {
                view: self.view,
                current: pos,
                open: true,
                viewport_id,
                initialized: false,
            });
        }
    }

    fn thumbnail_caption(&self, info: &ImageInfo) -> String {
        match &info.classification {
            Some(classification) => {
                let mut label = match &classification.decision {
                    Decision::Label(name) => self.display_for(name),
                    Decision::Unknown => "Leeg".to_string(),
                };
                if matches!(&classification.decision, Decision::Label(name) if name.ends_with(" (manueel)"))
                {
                    label.push_str(" (manueel)");
                }
                format!("{label} ({:.1}%)", classification.confidence * 100.0)
            }
            None => "Geen classificatie".to_string(),
        }
    }

    fn draw_thumbnail_card(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        idx: usize,
        is_selected: bool,
        loaded_this_frame: &mut usize,
    ) -> egui::Response {
        let (file_path, file_label, caption) = {
            let info = &self.rijen[idx];
            let label = info
                .file
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| info.file.to_string_lossy().to_string());
            let caption = self.thumbnail_caption(info);
            (info.file.clone(), label, caption)
        };

        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(CARD_WIDTH, CARD_HEIGHT), egui::Sense::click());

        let visuals = ui.visuals();
        let fill = if is_selected {
            visuals.selection.bg_fill
        } else {
            visuals.widgets.noninteractive.bg_fill
        };
        let stroke = if is_selected {
            visuals.selection.stroke
        } else {
            visuals.widgets.noninteractive.bg_stroke
        };
        ui.painter().rect_filled(rect, 8.0, fill);
        ui.painter()
            .rect_stroke(rect, 8.0, stroke, egui::StrokeKind::Outside);

        let builder = egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(8.0, 8.0)))
            .layout(egui::Layout::top_down(egui::Align::Center));
        let mut child = ui.new_child(builder);
        child.set_width(rect.width() - 16.0);
        child.label(egui::RichText::new(file_label.clone()).small());
        child.add_space(4.0);

        let had_tex = self.thumbs.contains_key(&file_path);
        let tex_id = if had_tex || *loaded_this_frame < MAX_THUMB_LOAD_PER_FRAME {
            let tex = self.get_or_load_thumb(ctx, &file_path);
            if tex.is_some() && !had_tex {
                *loaded_this_frame += 1;
            }
            tex
        } else {
            None
        };
        let image_size = egui::Vec2::splat(THUMB_SIZE as f32);
        if let Some(id) = tex_id {
            child.add(
                egui::Image::new((id, image_size))
                    .maintain_aspect_ratio(true)
                    .sense(egui::Sense::hover()),
            );
        } else {
            let (img_rect, _) = child.allocate_exact_size(image_size, egui::Sense::hover());
            child
                .painter()
                .rect_filled(img_rect, 4.0, egui::Color32::from_gray(40));
            child.painter().rect_stroke(
                img_rect,
                4.0,
                egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                egui::StrokeKind::Inside,
            );
        }

        child.add_space(4.0);
        child.label(egui::RichText::new(caption).small());

        let targets = self.context_targets(idx);
        response.context_menu(|ui| {
            self.render_context_menu(ui, &targets);
        });
        response
    }

    fn render_settings_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Instellingen");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let slider = egui::Slider::new(&mut self.pending_presence_threshold, 0.0..=1.0)
                .text("Onzekerheidsdrempel")
                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0));
            ui.add(slider);
            if ui.button("Herbereken").clicked() {
                self.presence_threshold = self.pending_presence_threshold;
                self.apply_presence_threshold();
                self.status = format!(
                    "Onzekerheidsdrempel toegepast: {:.0}%",
                    self.presence_threshold * 100.0
                );
                self.panel = Panel::Results;
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label("Batchgrootte");
            let resp = ui.add(
                egui::DragValue::new(&mut self.batch_size)
                    .range(1..=64)
                    .speed(1),
            );
            if resp.changed() {
                self.status = "Nieuwe batchgrootte wordt toegepast bij volgende scan".to_string();
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label("Achtergrondlabels");
            let response = ui.text_edit_singleline(&mut self.background_labels_input);
            if response.changed() {
                self.sync_background_labels();
                self.status = "Achtergrondlabels bijgewerkt voor huidige resultaten".to_string();
            }
        });

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(6.0);
        ui.checkbox(
            &mut self.improve_recognition,
            "Help de herkenning te verbeteren",
        );
        ui.label(
            "Wanneer je handmatig een categorie wijzigt, uploaden we die afbeeldingen op de achtergrond naar Roboflow.",
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Roboflow dataset (bijv. voederhuiscamera)");
            ui.text_edit_singleline(&mut self.roboflow_dataset_input);
        });
        ui.add_space(4.0);
        ui.label("Uploads gebruiken een ingebouwde Roboflow API-sleutel en draaien volledig op de achtergrond.");

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(6.0);
        ui.heading("Versies");
        ui.label(format!("App versie: {}", self.app_version));
        ui.label(format!(
            "Herkenningsmodel en soortenlijstversie: {}",
            self.model_version
        ));
        self.render_update_section(ui);
    }

    fn model_file_path(&self) -> PathBuf {
        self.model_root.join(MODEL_FILE_NAME)
    }

    fn labels_path(&self) -> PathBuf {
        self.model_root.join(LABEL_FILE_NAME)
    }

    fn model_version_path(&self) -> PathBuf {
        self.model_root.join(VERSION_FILE_NAME)
    }

    fn render_update_section(&mut self, ui: &mut egui::Ui) {
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
                    ui.hyperlink_to("Bekijk release", &summary.model_url);
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

    fn render_model_download_actions(&mut self, ui: &mut egui::Ui, summary: &UpdateSummary) {
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

    fn render_model_download_feedback(&self, ui: &mut egui::Ui) {
        match &self.model_download_status {
            ModelDownloadStatus::Success(msg) => {
                ui.label(msg);
            }
            ModelDownloadStatus::Error(err) => {
                ui.label(err);
            }
            _ => {}
        }
    }

    fn render_folder_panel(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        if let Some(path) = &self.gekozen_map {
            ui.label(format!("Fotomap: {}", path.display()));
            ui.label(format!("Afbeeldingen in deze map: {}", self.total_files));
        } else {
            ui.label("Geen fotomap geselecteerd.");
        }
        ui.add_space(8.0);
        if ui
            .add_enabled(!self.scan_in_progress, egui::Button::new("Map kiezen..."))
            .clicked()
            && let Some(dir) = FileDialog::new().set_directory(".").pick_folder()
        {
            self.set_selected_folder(dir);
        }
        let can_scan = self.gekozen_map.is_some() && !self.scan_in_progress;
        if ui
            .add_enabled(can_scan, egui::Button::new("Scannen"))
            .clicked()
            && let Some(dir) = self.gekozen_map.clone()
        {
            self.start_scan(dir);
            self.panel = Panel::Results;
        }
        if self.scan_in_progress {
            ui.add_space(8.0);
            self.render_progress_ui(ui);
        }
    }

    fn render_results_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.scan_in_progress {
            self.render_progress_ui(ui);
            return;
        }
        if !self.has_scanned {
            ui.label("Nog geen scan uitgevoerd.");
            return;
        }
        if self.rijen.is_empty() {
            ui.label("Geen resultaten beschikbaar.");
            return;
        }

        let (count_present, count_empty, count_unsure) = self.view_counts();
        ui.horizontal(|ui| {
            let present_btn = ui.selectable_label(
                self.view == ViewMode::Aanwezig,
                format!("Aanwezig ({count_present})"),
            );
            let empty_btn =
                ui.selectable_label(self.view == ViewMode::Leeg, format!("Leeg ({count_empty})"));
            let unsure_btn = ui.selectable_label(
                self.view == ViewMode::Onzeker,
                format!("Onzeker ({count_unsure})"),
            );
            if present_btn.clicked() {
                self.view = ViewMode::Aanwezig;
                self.thumbs.clear();
                self.thumb_keys.clear();
                self.selected_indices.clear();
                self.selection_anchor = None;
            }
            if empty_btn.clicked() {
                self.view = ViewMode::Leeg;
                self.thumbs.clear();
                self.thumb_keys.clear();
                self.selected_indices.clear();
                self.selection_anchor = None;
            }
            if unsure_btn.clicked() {
                self.view = ViewMode::Onzeker;
                self.thumbs.clear();
                self.thumb_keys.clear();
                self.selected_indices.clear();
                self.selection_anchor = None;
            }
        });

        let filtered = self.filtered_indices();
        self.handle_select_shortcuts(ctx, &filtered);

        if filtered.is_empty() {
            ui.label("Geen frames om te tonen in deze weergave.");
        } else {
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    let mut loaded_this_frame = 0usize;
                    ui.horizontal_wrapped(|ui| {
                        for &idx in &filtered {
                            let is_selected = self.selected_indices.contains(&idx);
                            let response = self.draw_thumbnail_card(
                                ui,
                                ctx,
                                idx,
                                is_selected,
                                &mut loaded_this_frame,
                            );
                            if response.clicked() {
                                let modifiers = ctx.input(|i| i.modifiers);
                                self.handle_selection_click(&filtered, idx, modifiers);
                            }
                            if response.double_clicked() {
                                self.open_preview(&filtered, idx);
                            }
                        }
                    });
                });
        }
    }

    fn render_progress_ui(&self, ui: &mut egui::Ui) {
        let total = self.total_files.max(1);
        let frac = (self.scanned_count as f32) / (total as f32);
        ui.add(egui::ProgressBar::new(frac).text(format!(
            "Scannen... {} / {} ({:.0}%)",
            self.scanned_count,
            self.total_files,
            frac * 100.0
        )));
    }

    fn set_selected_folder(&mut self, dir: PathBuf) {
        self.gekozen_map = Some(dir.clone());
        self.panel = Panel::Folder;
        self.rijen.clear();
        self.status.clear();
        self.has_scanned = false;
        self.scanned_count = 0;
        self.total_files = 0;
        self.view = ViewMode::Aanwezig;
        self.thumbs.clear();
        self.thumb_keys.clear();
        self.full_images.clear();
        self.full_keys.clear();
        match scan_folder_with(&dir, ScanOptions { recursive: false }) {
            Ok(rows) => {
                self.total_files = rows.len();
            }
            Err(e) => {
                self.status = format!("Fout bij lezen van map: {e}");
            }
        }
    }

    fn start_scan(&mut self, dir: PathBuf) {
        self.scan_in_progress = true;
        self.status = "Bezig met scannen...".to_string();
        self.scanned_count = 0;
        self.panel = Panel::Results;
        let (tx, rx): (Sender<ScanMsg>, Receiver<ScanMsg>) = mpsc::channel();
        self.rx = Some(rx);
        let cfg = self.classifier_config();
        thread::spawn(move || {
            let t0 = Instant::now();
            let mut rows = match scan_folder_with(&dir, ScanOptions { recursive: false }) {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(ScanMsg::Error(format!("Map scannen mislukt: {e}")));
                    tracing::warn!("scan_folder_with failed: {}", e);
                    return;
                }
            };
            let total = rows.len();
            let _ = tx.send(ScanMsg::Progress(0, total));
            let classifier = match EfficientVitClassifier::new(&cfg) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(ScanMsg::Error(format!("Model laden mislukt: {e}")));
                    return;
                }
            };
            let tx_progress = tx.clone();
            if let Err(e) = classifier.classify_with_progress(&mut rows, |done, total| {
                let _ = tx_progress.send(ScanMsg::Progress(done.min(total), total));
            }) {
                let _ = tx.send(ScanMsg::Error(format!("Classificatie mislukt: {e}")));
                return;
            }

            let _ = tx.send(ScanMsg::Progress(total, total));
            let elapsed_ms = t0.elapsed().as_millis();
            let _ = tx.send(ScanMsg::Done(rows, elapsed_ms));
        });
    }

    fn render_preview_window(&mut self, ctx: &egui::Context) {
        let Some(mut preview) = self.preview.take() else {
            return;
        };
        if !preview.open {
            return;
        }
        let indices = self.indices_for_view(preview.view);
        if indices.is_empty() {
            return;
        }
        if preview.current >= indices.len() {
            preview.current = indices.len() - 1;
        }
        let current_idx = indices[preview.current];
        let Some(info) = self.rijen.get(current_idx) else {
            return;
        };
        let file_name = info
            .file
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| info.file.to_string_lossy().to_string());
        let info_path = info.file.clone();
        let classification = info.classification.clone();
        let status_text = classification
            .as_ref()
            .map(|classification| match &classification.decision {
                Decision::Label(name) => {
                    if let Some(stripped) = name.strip_suffix(" (manueel)") {
                        format!("{stripped} (manueel)")
                    } else {
                        format!("{} ({:.1}%)", name, classification.confidence * 100.0)
                    }
                }
                Decision::Unknown => "Leeg".to_string(),
            })
            .unwrap_or_else(|| "Geen classificatie beschikbaar.".to_string());
        let full_tex = self.get_or_load_full_image(ctx, &info_path);
        let tex_info = full_tex.as_ref().map(|tex| (tex.id(), tex.size_vec2()));
        let viewport_id = preview.viewport_id;
        let mut builder = egui::ViewportBuilder::default().with_title(file_name.clone());
        if !preview.initialized {
            builder = builder.with_inner_size([640.0, 480.0]);
        }
        let mut action = PreviewAction::None;
        let status_panel_id = format!("preview-status-{viewport_id:?}");
        let current_targets = vec![current_idx];
        ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
            let mut wants_prev = false;
            let mut wants_next = false;
            ctx.input(|input| {
                for event in &input.events {
                    if let egui::Event::Key {
                        key: egui::Key::ArrowLeft,
                        pressed: true,
                        ..
                    } = event
                    {
                        wants_prev = true;
                    } else if let egui::Event::Key {
                        key: egui::Key::ArrowRight,
                        pressed: true,
                        ..
                    } = event
                    {
                        wants_next = true;
                    }
                }
            });
            if ctx.input(|i| i.viewport().close_requested()) {
                action = PreviewAction::Close;
            }
            egui::TopBottomPanel::bottom(status_panel_id.clone())
                .resizable(false)
                .show(ctx, |ui| {
                    let response =
                        ui.add(egui::Label::new(status_text.clone()).sense(egui::Sense::click()));
                    response.context_menu(|ui| {
                        self.render_context_menu(ui, &current_targets);
                    });
                });
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let prev_disabled = preview.current == 0;
                    if ui
                        .add_enabled(!prev_disabled, egui::Button::new("◀ Vorige"))
                        .clicked()
                    {
                        action = PreviewAction::Prev;
                    }
                    if wants_prev && !prev_disabled {
                        action = PreviewAction::Prev;
                    }
                    let next_disabled = preview.current + 1 >= indices.len();
                    if ui
                        .add_enabled(!next_disabled, egui::Button::new("Volgende ▶"))
                        .clicked()
                    {
                        action = PreviewAction::Next;
                    }
                    if wants_next && !next_disabled {
                        action = PreviewAction::Next;
                    }
                    ui.label(format!("{} / {}", preview.current + 1, indices.len()));
                });
                ui.separator();
                if let Some((tex_id, tex_size)) = tex_info {
                    let avail = ui.available_size();
                    let scale = (avail.x / tex_size.x).min(avail.y / tex_size.y).max(0.01);
                    let draw_size = tex_size * scale;
                    let inner = ui.allocate_ui_with_layout(
                        avail,
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            ui.add(
                                egui::Image::new((tex_id, tex_size))
                                    .fit_to_exact_size(draw_size)
                                    .sense(egui::Sense::click()),
                            )
                        },
                    );
                    inner.inner.context_menu(|ui| {
                        self.render_context_menu(ui, &current_targets);
                    });
                } else {
                    ui.label("Afbeelding kon niet geladen worden.");
                }
            });
        });
        preview.initialized = true;
        match action {
            PreviewAction::Prev => {
                if preview.current > 0 {
                    preview.current -= 1;
                }
            }
            PreviewAction::Next => {
                if preview.current + 1 < indices.len() {
                    preview.current += 1;
                }
            }
            PreviewAction::Close => preview.open = false,
            PreviewAction::None => {}
        }
        if preview.open {
            self.preview = Some(preview);
        }
    }

    fn apply_presence_threshold(&mut self) {
        let threshold = self.presence_threshold;
        let backgrounds = &self.background_labels;
        for info in &mut self.rijen {
            let mut present = false;
            if let Some(classification) = &info.classification {
                match &classification.decision {
                    Decision::Label(name) => {
                        let lower = name.to_ascii_lowercase();
                        if !backgrounds.iter().any(|bg| bg == &lower) {
                            present = classification.confidence >= threshold;
                        }
                    }
                    Decision::Unknown => present = false,
                }
            }
            info.present = present;
        }
    }

    fn is_background_label(&self, name_lower: &str) -> bool {
        self.background_labels.iter().any(|bg| bg == name_lower)
    }

    fn is_onzeker(&self, info: &ImageInfo) -> bool {
        let Some(classification) = &info.classification else {
            return false;
        };
        match &classification.decision {
            Decision::Label(name) => {
                let lower = canonical_label(name);
                if lower == "iets sp" {
                    return true;
                }
                if self.is_background_label(&lower) {
                    return false;
                }
                classification.confidence < self.presence_threshold
            }
            Decision::Unknown => false,
        }
    }

    fn sync_background_labels(&mut self) {
        let parsed: Vec<String> = self
            .background_labels_input
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        self.background_labels = if parsed.is_empty() {
            vec!["achtergrond".to_string()]
        } else {
            parsed
        };
        if !self.rijen.is_empty() {
            self.apply_presence_threshold();
        }
    }

    fn classifier_config(&self) -> ClassifierConfig {
        ClassifierConfig {
            model_path: self.model_file_path(),
            labels_path: self.labels_path(),
            presence_threshold: self.pending_presence_threshold,
            batch_size: self.batch_size.max(1),
            background_labels: self.background_labels.clone(),
            ..Default::default()
        }
    }
}

impl App for UiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        while let Ok(msg) = self.upload_status_rx.try_recv() {
            self.status = msg;
        }
        self.poll_manifest_updates();
        self.poll_model_download();
        // Drain scan messages first
        if let Some(rx) = self.rx.take() {
            let mut keep = true;
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    ScanMsg::Progress(done, total) => {
                        self.scanned_count = done.min(total);
                        self.total_files = total;
                    }
                    ScanMsg::Done(rows, elapsed_ms) => {
                        self.scan_in_progress = false;
                        self.has_scanned = true;
                        self.rijen = rows;
                        self.thumbs.clear();
                        self.thumb_keys.clear();
                        self.presence_threshold = self.pending_presence_threshold;
                        self.apply_presence_threshold();
                        self.selected_indices.clear();
                        self.selection_anchor = None;
                        let totaal = self.total_files;
                        let (count_present, _, _) = self.view_counts();
                        self.status = format!(
                            "Gereed: Dieren gevonden in {count_present} van {totaal} frames ({:.1} s)",
                            (elapsed_ms as f32) / 1000.0
                        );
                        keep = false;
                        break;
                    }
                    ScanMsg::Error(message) => {
                        self.scan_in_progress = false;
                        self.has_scanned = false;
                        self.status = message;
                        keep = false;
                        break;
                    }
                }
            }
            if keep {
                self.rx = Some(rx);
            }
        }
        if self.scan_in_progress || self.rx.is_some() {
            // Keep egui polling so background progress channels are drained.
            ctx.request_repaint();
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new("Fotomap").selected(self.panel == Panel::Folder))
                    .clicked()
                {
                    self.panel = Panel::Folder;
                }
                let can_view_results = self.has_scanned || self.scan_in_progress;
                if ui
                    .add_enabled(
                        can_view_results,
                        egui::Button::new("Scanresultaat").selected(self.panel == Panel::Results),
                    )
                    .clicked()
                {
                    self.panel = Panel::Results;
                }
                let can_view_export =
                    self.has_scanned && !self.rijen.is_empty() && !self.scan_in_progress;
                if ui
                    .add_enabled(
                        can_view_export,
                        egui::Button::new("Exporteren").selected(self.panel == Panel::Export),
                    )
                    .clicked()
                {
                    self.panel = Panel::Export;
                }
                if ui
                    .add(egui::Button::new("Instellingen").selected(self.panel == Panel::Settings))
                    .clicked()
                {
                    self.panel = Panel::Settings;
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.panel {
            Panel::Folder => self.render_folder_panel(ui, ctx),
            Panel::Results => self.render_results_panel(ui, ctx),
            Panel::Export => self.render_export_panel(ui),
            Panel::Settings => self.render_settings_panel(ui),
        });

        self.render_preview_window(ctx);
        self.render_coordinate_prompt(ctx);

        let status_display = if self.status.is_empty() {
            if self.scan_in_progress {
                "Bezig met scannen...".to_string()
            } else if self.has_scanned {
                "Gereed.".to_string()
            } else {
                "Klaar.".to_string()
            }
        } else {
            self.status.clone()
        };
        egui::TopBottomPanel::bottom("status-bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(status_display);
            });
        });
    }
}

impl UiApp {
    fn load_label_options_from(path: &Path) -> Vec<LabelOption> {
        let Ok(content) = std::fs::read_to_string(path) else {
            tracing::warn!(
                "Kon labels niet laden uit {}: bestand ontbreekt of is onleesbaar",
                path.display()
            );
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut options = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let (display, scientific_raw) = match trimmed.split_once(',') {
                Some((name, sci)) => (name.trim(), sci.trim()),
                None => (trimmed, ""),
            };
            if display.is_empty() {
                continue;
            }
            let canonical = canonical_label(display);
            if canonical.is_empty() || !seen.insert(canonical.clone()) {
                continue;
            }
            options.push(LabelOption {
                canonical,
                display: display.to_string(),
                scientific: if scientific_raw.is_empty() {
                    None
                } else {
                    Some(scientific_raw.to_string())
                },
            });
        }
        options
    }

    fn prepare_model_dir() -> (PathBuf, String) {
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

    fn ensure_models_present(target: &Path) -> anyhow::Result<()> {
        if target.exists() {
            return Ok(());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Kon map {} niet aanmaken", parent.display()))?;
        }
        copy_dir_recursive(&bundled_models_dir(), target)
    }

    fn request_manifest_refresh(&mut self) {
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

    fn poll_manifest_updates(&mut self) {
        if let Some(rx) = self.update_rx.take() {
            match rx.try_recv() {
                Ok(Ok(manifest)) => self.apply_manifest(manifest),
                Ok(Err(err)) => self.manifest_status = ManifestStatus::Error(err),
                Err(TryRecvError::Empty) => {
                    self.update_rx = Some(rx);
                }
                Err(TryRecvError::Disconnected) => {
                    self.manifest_status =
                        ManifestStatus::Error("Zoeken naar updates is mislukt".to_string());
                }
            }
        }
    }

    fn apply_manifest(&mut self, manifest: RemoteManifest) {
        let mut summary = UpdateSummary {
            latest_app: manifest.app.latest.clone(),
            app_url: manifest.app.url.clone(),
            latest_model: manifest.model.latest.clone(),
            model_url: manifest.model.url.clone(),
            model_size_mb: manifest.model.size_mb,
            model_notes: manifest.model.notes.clone(),
            ..Default::default()
        };
        summary.app_update_available = version_is_newer(&manifest.app.latest, &self.app_version);
        summary.model_update_available = manifest.model.latest.trim() != self.model_version.trim();
        if !summary.model_update_available {
            self.model_download_status = ModelDownloadStatus::Idle;
        }
        self.manifest_status = ManifestStatus::Ready(summary);
    }

    fn start_model_download(&mut self, summary: &UpdateSummary) {
        if matches!(self.model_download_status, ModelDownloadStatus::Downloading) {
            return;
        }
        let url = summary.model_url.clone();
        let version = summary.latest_model.clone();
        let target_root = self.model_root.clone();
        let (tx, rx) = mpsc::channel();
        self.model_download_rx = Some(rx);
        self.model_download_status = ModelDownloadStatus::Downloading;
        thread::spawn(move || {
            let result = download_and_install_model(&url, &target_root, &version)
                .map(|_| version.clone())
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }

    fn poll_model_download(&mut self) {
        if let Some(rx) = self.model_download_rx.take() {
            match rx.try_recv() {
                Ok(Ok(version)) => {
                    self.model_download_status =
                        ModelDownloadStatus::Success(format!("Model {version} geïnstalleerd."));
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

    fn render_context_menu(&mut self, ui: &mut egui::Ui, indices: &[usize]) {
        ui.menu_button("Exporteren", |ui| {
            ui.close();
            self.export_selected_images(indices);
        });
        ui.separator();
        if ui.button("Markeer als Achtergrond (Leeg)").clicked() {
            self.assign_manual_category(indices, "achtergrond".into(), false);
            ui.close();
        }
        if ui.button("Markeer als Iets sp. (Onzeker)").clicked() {
            self.assign_manual_category(indices, "iets sp".into(), false);
            ui.close();
        }
        ui.separator();
        for label in self.available_labels() {
            let display = self.display_for(&label);
            if ui.button(display).clicked() {
                self.assign_manual_category(indices, label, true);
                ui.close();
            }
        }
        ui.separator();
        ui.menu_button("Nieuw...", |ui| {
            ui.label("Vul een nieuwe soortnaam in:");
            ui.horizontal(|ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.new_label_buffer)
                        .hint_text("Nieuwe soort"),
                );
                resp.request_focus();
                let mut submit = false;
                if resp.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !self.new_label_buffer.trim().is_empty()
                {
                    submit = true;
                }
                if ui.button("OK").clicked() {
                    submit = true;
                }
                if submit && self.apply_new_label(indices) {
                    ui.close();
                }
            });
        });
    }

    fn render_export_panel(&mut self, ui: &mut egui::Ui) {
        if !self.has_scanned || self.rijen.is_empty() {
            ui.label("Er zijn nog geen scanresultaten om te exporteren.");
            return;
        }

        ui.heading("Opties");
        ui.add_space(4.0);
        ui.checkbox(
            &mut self.export_present,
            "Exporteer foto's met aanwezige soorten",
        );
        ui.checkbox(
            &mut self.export_uncertain,
            "Exporteer foto's met onzekere identificatie",
        );
        ui.checkbox(
            &mut self.export_background,
            "Exporteer foto's uit Leeg (achtergrond)",
        );
        let csv_checkbox = ui.checkbox(
            &mut self.export_csv,
            "Exporteer identificatieresultaten als CSV bestand",
        );
        if csv_checkbox.clicked() && self.export_csv {
            self.export_present = true;
        }

        ui.add_space(12.0);
        let can_export = self.can_export_from_panel();
        let button = ui.add_enabled(can_export, egui::Button::new("Exporteer"));
        if button.clicked() {
            self.start_export_workflow();
        }
        if !can_export {
            ui.label("Selecteer minstens één categorie om te exporteren.");
        }
    }

    fn can_export_from_panel(&self) -> bool {
        self.has_scanned
            && !self.rijen.is_empty()
            && (self.export_present || self.export_uncertain || self.export_background)
    }

    fn start_export_workflow(&mut self) {
        if !self.can_export_from_panel() {
            self.status = "Geen foto's om te exporteren.".to_string();
            return;
        }
        if self.export_csv && !self.export_present {
            self.status =
                "CSV export vereist dat 'aanwezige soorten' wordt meegekopieerd.".to_string();
            return;
        }

        let mut dialog = FileDialog::new();
        if let Some(dir) = &self.gekozen_map {
            dialog = dialog.set_directory(dir);
        }
        let Some(target_dir) = dialog.pick_folder() else {
            self.status = "Export geannuleerd.".to_string();
            return;
        };

        let options = ExportOptions {
            include_present: self.export_present,
            include_uncertain: self.export_uncertain,
            include_background: self.export_background,
            include_csv: self.export_csv,
        };
        let pending = PendingExport {
            target_dir,
            options,
        };

        if pending.options.include_csv {
            self.pending_export = Some(pending);
            self.coordinate_prompt = Some(CoordinatePrompt::default());
        } else {
            let result = self.perform_export(pending, None);
            self.handle_export_result(result);
        }
    }

    fn handle_export_result(&mut self, result: anyhow::Result<ExportOutcome>) {
        match result {
            Ok(summary) => {
                let mut message = if summary.copied == 0 {
                    format!(
                        "Geen bestanden geëxporteerd in {}",
                        summary.target_dir.display()
                    )
                } else {
                    format!(
                        "{} foto('s) geëxporteerd naar {}",
                        summary.copied,
                        summary.target_dir.display()
                    )
                };
                if summary.wrote_csv {
                    message.push_str("; CSV opgeslagen.");
                }
                self.status = message;
            }
            Err(err) => {
                self.status = format!("Exporteren mislukt: {err}");
            }
        }
    }

    fn complete_pending_export(&mut self, coords: (f64, f64)) {
        if let Some(pending) = self.pending_export.take() {
            let result = self.perform_export(pending, Some(coords));
            self.handle_export_result(result);
        }
        self.coordinate_prompt = None;
    }

    fn render_coordinate_prompt(&mut self, ctx: &egui::Context) {
        if self.coordinate_prompt.is_none() {
            return;
        }
        ctx.request_repaint();

        let mut close_requested = false;
        let mut submit_coords: Option<(f64, f64)> = None;

        {
            let prompt = self.coordinate_prompt.as_mut().unwrap();
            let mut open = true;
            egui::Window::new("Coördinaten voor CSV")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                ui.label("Plak hier de Google Maps coördinaten:");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut prompt.input)
                        .desired_width(260.0)
                        .hint_text("51.376318769269716, 4.456974517090091"),
                );
                response.context_menu(|ui| {
                    if ui.button("Plakken").clicked() {
                        match Clipboard::new().and_then(|mut cb| cb.get_text()) {
                            Ok(text) => {
                                prompt.input = text;
                                prompt.error = None;
                            }
                            Err(err) => {
                                prompt.error = Some(format!("Plakken mislukt: {err}"));
                            }
                        }
                        ui.close();
                    }
                });

                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                        ui.label("Tip: open ");
                        ui.hyperlink_to("Google Maps", "https://maps.google.com");
                        ui.label(
                            " en klik met de rechtermuisknop op de plaats van de camera. Klik vervolgens op de coördinaten bovenaan het verschenen keuzemenu. Deze worden automatisch naar het klembord gekopieerd. Klik opnieuw met de rechtermuisknop in het veld hierboven en kies plakken.",
                        );
                    });

                    if let Some(err) = &prompt.error {
                        ui.add_space(4.0);
                        ui.colored_label(egui::Color32::RED, err);
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Annuleer").clicked() {
                            close_requested = true;
                        }
                        let mut submit = false;
                        if ui.button("Opslaan").clicked() {
                            submit = true;
                        }
                        if response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            submit = true;
                        }
                        if submit {
                            match parse_coordinates(&prompt.input) {
                                Ok(coords) => {
                                    submit_coords = Some(coords);
                                }
                                Err(err) => {
                                    prompt.error = Some(err.to_string());
                                }
                            }
                        }
                    });
                });

            if !open {
                close_requested = true;
            }
        }

        if let Some(coords) = submit_coords {
            self.complete_pending_export(coords);
        } else if close_requested {
            self.pending_export = None;
            self.coordinate_prompt = None;
            self.status = "Export geannuleerd.".to_string();
        }
    }

    fn export_selected_images(&mut self, indices: &[usize]) {
        if indices.is_empty() {
            self.status = "Geen foto's geselecteerd voor export.".to_string();
            return;
        }

        let mut dialog = FileDialog::new();
        if let Some(dir) = &self.gekozen_map {
            dialog = dialog.set_directory(dir);
        }

        let Some(target_dir) = dialog.pick_folder() else {
            self.status = "Export geannuleerd.".to_string();
            return;
        };

        match self.copy_selection_to(&target_dir, indices) {
            Ok(0) => {
                self.status =
                    "Geen export uitgevoerd: geen bruikbare bestanden gevonden.".to_string();
            }
            Ok(count) => {
                self.status = format!(
                    "{count} foto('s) geëxporteerd naar {}",
                    target_dir.display()
                );
            }
            Err(err) => {
                self.status = format!("Exporteren mislukt: {err}");
            }
        }
    }

    fn copy_selection_to(&self, target_dir: &Path, indices: &[usize]) -> anyhow::Result<usize> {
        use anyhow::Context;

        let mut copied = 0usize;
        for &idx in indices {
            let Some(info) = self.rijen.get(idx) else {
                continue;
            };
            let label = self.label_for_export(info);
            let folder_name = sanitize_for_path(&label);
            if folder_name.is_empty() {
                continue;
            }
            let label_dir = target_dir.join(&folder_name);
            fs::create_dir_all(&label_dir)
                .with_context(|| format!("Kon map {} niet aanmaken", label_dir.display()))?;

            let sanitized_label = sanitize_for_path(&label);
            let stem = info
                .file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("image");
            let sanitized_stem = sanitize_for_path(stem);
            let base_name = if sanitized_stem.is_empty() {
                sanitized_label.clone()
            } else {
                format!("{sanitized_label}_{sanitized_stem}")
            };
            let dest_path = next_available_export_path(&label_dir, &base_name, "jpg");
            fs::copy(&info.file, &dest_path).with_context(|| {
                format!(
                    "Kopiëren van {} naar {} mislukt",
                    info.file.display(),
                    dest_path.display()
                )
            })?;
            copied += 1;
        }

        Ok(copied)
    }

    fn label_for_export(&self, info: &ImageInfo) -> String {
        match info.classification.as_ref().map(|c| &c.decision) {
            Some(Decision::Label(name)) => self.display_for(name),
            Some(Decision::Unknown) => "Onbekend".to_string(),
            None => {
                if info.present {
                    "Onbekend".to_string()
                } else {
                    "Leeg".to_string()
                }
            }
        }
    }

    fn available_labels(&self) -> Vec<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut ordered = Vec::new();
        for option in &self.label_options {
            if option.canonical == "achtergrond" || option.canonical == "iets sp" {
                continue;
            }
            if seen.insert(option.canonical.clone()) {
                ordered.push(option.canonical.clone());
            }
        }
        for info in &self.rijen {
            if let Some(classification) = &info.classification
                && let Decision::Label(name) = &classification.decision
            {
                let canonical = canonical_label(name);
                if canonical == "achtergrond" || canonical == "iets sp" {
                    continue;
                }
                if seen.insert(canonical.clone()) {
                    ordered.push(canonical);
                }
            }
        }
        ordered
    }

    fn perform_export(
        &self,
        pending: PendingExport,
        coords: Option<(f64, f64)>,
    ) -> anyhow::Result<ExportOutcome> {
        use anyhow::{Context, anyhow};

        let PendingExport {
            target_dir,
            options,
        } = pending;
        if options.include_csv && coords.is_none() {
            return Err(anyhow!("Coördinaten ontbreken voor CSV-export"));
        }

        let jobs = self.collect_export_jobs(&options);
        if jobs.is_empty() && !options.include_csv {
            return Err(anyhow!("Geen bestanden voldeden aan de huidige selectie."));
        }

        let mut copied = 0usize;
        let mut csv_records: Vec<CsvRecord> = Vec::new();
        let export_time = Local::now();

        for job in jobs {
            let folder_name = sanitize_for_path(&job.folder_label);
            if folder_name.is_empty() {
                continue;
            }
            let folder_path = target_dir.join(&folder_name);
            fs::create_dir_all(&folder_path)
                .with_context(|| format!("Kon map {} niet aanmaken", folder_path.display()))?;

            let stem = job
                .source
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("image");
            let sanitized_stem = sanitize_for_path(stem);
            let base = if sanitized_stem.is_empty() {
                folder_name.clone()
            } else {
                format!("{folder_name}_{sanitized_stem}")
            };
            let dest_path = next_available_export_path(&folder_path, &base, "jpg");
            fs::copy(&job.source, &dest_path).with_context(|| {
                format!(
                    "Kopiëren van {} naar {} mislukt",
                    job.source.display(),
                    dest_path.display()
                )
            })?;

            if job.include_in_csv {
                let (date, time) = extract_timestamp(&job.source)?;
                let canonical = job
                    .canonical_label
                    .clone()
                    .unwrap_or_else(|| canonical_label(&job.folder_label));
                let scientific = self
                    .scientific_for(&canonical)
                    .unwrap_or_else(|| job.folder_label.clone());
                csv_records.push(CsvRecord {
                    date,
                    time,
                    scientific,
                    path: dest_path.to_string_lossy().into_owned(),
                });
                // coords reused later when writing file
            }

            copied += 1;
        }

        if options.include_csv {
            let coords = coords.unwrap();
            write_export_csv(&target_dir, &csv_records, coords, export_time)?;
        }

        Ok(ExportOutcome {
            copied,
            wrote_csv: options.include_csv,
            target_dir,
        })
    }

    fn collect_export_jobs(&self, options: &ExportOptions) -> Vec<ExportJob> {
        let mut jobs = Vec::new();
        for info in &self.rijen {
            if options.include_present
                && info.present
                && let Some((display, canonical)) = self.present_label(info)
            {
                jobs.push(ExportJob {
                    source: info.file.clone(),
                    folder_label: display,
                    canonical_label: Some(canonical),
                    include_in_csv: options.include_csv,
                });
            }
            if options.include_uncertain && self.is_onzeker(info) {
                jobs.push(ExportJob {
                    source: info.file.clone(),
                    folder_label: "Onzeker".to_string(),
                    canonical_label: None,
                    include_in_csv: false,
                });
            }
            if options.include_background && self.belongs_in_leeg(info) {
                jobs.push(ExportJob {
                    source: info.file.clone(),
                    folder_label: "Leeg".to_string(),
                    canonical_label: None,
                    include_in_csv: false,
                });
            }
        }
        jobs
    }

    fn present_label(&self, info: &ImageInfo) -> Option<(String, String)> {
        let classification = info.classification.as_ref()?;
        if let Decision::Label(name) = &classification.decision {
            let canonical = canonical_label(name);
            if self.is_background_label(&canonical) || canonical == "iets sp" {
                return None;
            }
            let display = self.display_for(&canonical);
            return Some((display, canonical));
        }
        None
    }

    fn belongs_in_leeg(&self, info: &ImageInfo) -> bool {
        !info.present && !self.is_onzeker(info)
    }

    fn scientific_for(&self, canonical: &str) -> Option<String> {
        self.label_options
            .iter()
            .find(|option| option.canonical == canonical)
            .and_then(|option| option.scientific.clone())
    }

    fn assign_manual_category(&mut self, indices: &[usize], label: String, mark_present: bool) {
        let lower = canonical_label(&label);
        let display = self.display_for(&lower);
        let mut paths: Vec<PathBuf> = Vec::new();
        for &idx in indices {
            if let Some(info) = self.rijen.get_mut(idx) {
                info.classification = Some(Classification {
                    decision: Decision::Label(format!("{display} (manueel)")),
                    confidence: 1.0,
                });
                info.present = mark_present && lower != "achtergrond";
                paths.push(info.file.clone());
            }
        }
        self.status = format!("{} kaart(en) gemarkeerd als {}", indices.len(), display);

        // Background upload to Roboflow if enabled and configured
        if self.improve_recognition {
            let dataset = self
                .roboflow_dataset_input
                .trim()
                .trim_matches('/')
                .to_string();
            let label_for_upload = display.clone();
            let api_key = ROBOFLOW_API_KEY.trim();
            if api_key.is_empty() {
                self.status =
                    "Roboflow upload staat aan, maar er is geen API-sleutel ingebouwd.".to_string();
            } else if dataset.is_empty() {
                self.status = "Roboflow upload niet uitgevoerd: dataset ontbreekt.".to_string();
            } else if paths.is_empty() {
                self.status =
                    "Roboflow upload niet uitgevoerd: geen foto's geselecteerd.".to_string();
            } else {
                let upload_count = paths.len();
                let status_tx = self.upload_status_tx.clone();
                self.status = "Foto('s) met manuele identificatie worden geüpload...".to_string();
                std::thread::spawn(move || {
                    let mut last_err: Option<String> = None;
                    for path in paths {
                        if let Err(e) =
                            upload_to_roboflow(&path, &label_for_upload, &dataset, api_key)
                        {
                            last_err = Some(e.to_string());
                            break;
                        }
                    }
                    let message = if let Some(err) = last_err {
                        format!("Upload van foto('s) met manuele identificatie mislukt: {err}")
                    } else if upload_count == 1 {
                        "Foto met manuele identificatie geüpload.".to_string()
                    } else {
                        format!("{upload_count} foto's met manuele identificatie geüpload.")
                    };
                    let _ = status_tx.send(message);
                });
            }
        }
    }

    fn apply_new_label(&mut self, indices: &[usize]) -> bool {
        let trimmed = self.new_label_buffer.trim();
        if trimmed.is_empty() {
            self.status = "Geen label ingevuld.".to_string();
            return false;
        }
        let new_label = trimmed.to_string();
        let canonical = canonical_label(&new_label);
        if canonical.is_empty() {
            self.status = "Label is ongeldig.".to_string();
            return false;
        }
        if !self
            .label_options
            .iter()
            .any(|option| option.canonical == canonical)
        {
            self.label_options.push(LabelOption {
                canonical: canonical.clone(),
                display: new_label.clone(),
                scientific: None,
            });
        }
        self.assign_manual_category(indices, new_label, true);
        self.new_label_buffer.clear();
        true
    }

    fn context_targets(&self, idx: usize) -> Vec<usize> {
        if self.selected_indices.contains(&idx) && !self.selected_indices.is_empty() {
            self.selected_indices.iter().copied().collect()
        } else {
            vec![idx]
        }
    }

    fn display_for(&self, name: &str) -> String {
        let canonical = canonical_label(name);
        if canonical == "iets sp" {
            return "Iets sp.".to_string();
        }
        if canonical == "achtergrond" {
            return "Achtergrond".to_string();
        }
        if let Some(option) = self
            .label_options
            .iter()
            .find(|option| option.canonical == canonical)
        {
            return option.display.clone();
        }
        fallback_display_label(&canonical)
    }
}

fn canonical_label(name: &str) -> String {
    let stripped = name.strip_suffix(" (manueel)").unwrap_or(name).trim();
    let primary = stripped
        .split_once(',')
        .map(|(first, _)| first.trim())
        .unwrap_or(stripped);
    let cleaned = primary.trim_end_matches(['.', ',']).trim();
    cleaned.to_ascii_lowercase()
}

fn load_app_icon() -> IconData {
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

fn bundled_models_dir() -> PathBuf {
    PathBuf::from("models")
}

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
                    "Kopiëren van {} naar {} mislukt",
                    entry.path().display(),
                    dest_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn read_model_version_from(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                "onbekend".to_string()
            } else {
                trimmed.to_string()
            }
        }
        Err(err) => {
            tracing::warn!("Kon modelversie niet lezen uit {}: {err}", path.display());
            "onbekend".to_string()
        }
    }
}

fn fallback_display_label(name: &str) -> String {
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

fn version_is_newer(latest: &str, current: &str) -> bool {
    match (Version::parse(latest), Version::parse(current)) {
        (Ok(lat), Ok(curr)) => lat > curr,
        _ => latest != current,
    }
}

fn sanitize_for_path(input: &str) -> String {
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

fn next_available_export_path(base_dir: &Path, base: &str, ext: &str) -> PathBuf {
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

fn extract_timestamp(path: &Path) -> anyhow::Result<(String, String)> {
    use anyhow::Context;

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

fn write_export_csv(
    dir: &Path,
    records: &[CsvRecord],
    coords: (f64, f64),
    export_time: DateTime<Local>,
) -> anyhow::Result<PathBuf> {
    use anyhow::Context;

    let base = format!("voederhuiscamera_{}", export_time.format("%y%m%d%H%M"));
    let csv_path = next_available_export_path(dir, &base, "csv");
    let mut writer = csv::Writer::from_path(&csv_path)
        .with_context(|| format!("Kon CSV-bestand {} niet openen", csv_path.display()))?;
    writer.write_record(["date", "time", "scientific name", "lat", "lng", "path"])?;
    let lat_str = format!("{}", coords.0);
    let lng_str = format!("{}", coords.1);
    for record in records {
        writer.write_record([
            record.date.as_str(),
            record.time.as_str(),
            record.scientific.as_str(),
            lat_str.as_str(),
            lng_str.as_str(),
            record.path.as_str(),
        ])?;
    }
    writer.flush()?;
    Ok(csv_path)
}

fn parse_coordinates(input: &str) -> anyhow::Result<(f64, f64)> {
    use anyhow::{Context, anyhow};

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

fn fetch_remote_manifest() -> anyhow::Result<RemoteManifest> {
    let client = reqwest::blocking::Client::builder()
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

fn download_and_install_model(url: &str, target_root: &Path, version: &str) -> anyhow::Result<()> {
    let client = reqwest::blocking::Client::builder()
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
                "Kopiëren van {} naar {} mislukt",
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

#[derive(Debug, Deserialize)]
struct RemoteManifest {
    app: ManifestEntry,
    model: ModelManifestEntry,
}

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    latest: String,
    url: String,
}

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

fn upload_to_roboflow(
    path: &Path,
    label: &str,
    dataset: &str,
    api_key: &str,
) -> anyhow::Result<()> {
    use anyhow::{Context, anyhow};
    use reqwest::blocking::{Client, multipart};
    use std::time::Duration;

    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());

    let dataset_slug = dataset.trim_matches('/');
    if dataset_slug.is_empty() {
        return Err(anyhow!("Roboflow datasetnaam ontbreekt"));
    }
    let dataset_slug_encoded = urlencoding::encode(dataset_slug);

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("HTTP client bouwen")?;

    let upload_url = format!(
        "https://api.roboflow.com/dataset/{}/upload?api_key={}&name={}&split=train",
        dataset_slug_encoded,
        api_key,
        urlencoding::encode(&filename)
    );

    let form = multipart::Form::new()
        .file("file", path)
        .with_context(|| format!("Bestand toevoegen aan upload-formulier: {}", path.display()))?;

    let response = client
        .post(&upload_url)
        .multipart(form)
        .send()
        .context("Roboflow-upload mislukt")?
        .error_for_status()
        .context("Roboflow-upload gaf een foutstatus")?;

    let json: serde_json::Value = response
        .json()
        .context("Uploadantwoord kon niet gelezen worden")?;
    let upload_id = json
        .get("id")
        .and_then(|id| id.as_str())
        .or_else(|| {
            json.get("image")
                .and_then(|img| img.get("id"))
                .and_then(|id| id.as_str())
        })
        .ok_or_else(|| anyhow!("Upload-ID ontbreekt in Roboflow-antwoord: {json}"))?;
    tracing::info!("Roboflow-upload voltooid ({upload_id})");

    // Attach a CSV classification annotation (filename,label) so Roboflow applies
    // the selected label without inventing new categories.
    let annotate_url = format!(
        "https://api.roboflow.com/dataset/{}/annotate/{}?api_key={}&name={}",
        dataset_slug_encoded,
        urlencoding::encode(upload_id),
        api_key,
        urlencoding::encode("classification.csv")
    );
    let annotation_text = format!("{label}\n");

    client
        .post(&annotate_url)
        .header("Content-Type", "text/plain")
        .body(annotation_text)
        .send()
        .context("Roboflow-annotatie mislukt")?
        .error_for_status()
        .context("Roboflow-annotatie gaf een foutstatus")?;

    Ok(())
}
