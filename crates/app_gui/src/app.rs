//! Core application state for the Feedie GUI.

use crate::export::{CoordinatePrompt, PendingExport};
use crate::manifest::{ManifestStatus, ModelDownloadStatus};
use eframe::{App, Frame, egui};
use feeder_core::ImageInfo;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

mod cache;
mod folder;
mod frame;
mod preview;
mod results;
mod selection;
mod settings;
mod thumbnails;

use self::preview::PreviewState;

/// Determines which subset of images is visible in the results grid.
///
/// Each mode filters the classifier results differently and drives the counters
/// shown at the top of the “Scanresultaat” panel.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewMode {
    #[default]
    Aanwezig,
    Leeg,
    Onzeker,
}

/// Identifies the panel that is currently shown in the top navigation bar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Panel {
    Folder,
    Results,
    Export,
    Settings,
}

/// Entry from the label CSV file that powers manual selections and exports.
#[derive(Clone)]
pub(crate) struct LabelOption {
    pub(crate) canonical: String,
    pub(crate) display: String,
    pub(crate) scientific: Option<String>,
}

/// Root egui application state that wires together all modules.
///
/// The `UiApp` is owned by `eframe` and persists for the lifetime of the
/// application. It stores user choices, scan results, textures, background task
/// channels, and metadata such as version information.
///
/// # Examples
///
/// ```
/// // Constructing UiApp uses defaults and requests the manifest in the background.
/// let mut app = feedie::UiApp::default();
/// // In tests you can toggle panels directly.
/// app.panel = feedie::Panel::Results;
/// ```
pub struct UiApp {
    pub(crate) gekozen_map: Option<PathBuf>,
    pub(crate) rijen: Vec<ImageInfo>,
    pub(crate) total_files: usize,
    pub(crate) scanned_count: usize,
    pub(crate) has_scanned: bool,
    pub(crate) scan_in_progress: bool,
    pub(crate) status: String,
    pub(crate) view: ViewMode,
    pub(crate) panel: Panel,
    pub(crate) rx: Option<Receiver<ScanMsg>>,
    pub(crate) thumbs: HashMap<PathBuf, egui::TextureHandle>,
    pub(crate) thumb_keys: VecDeque<PathBuf>,
    pub(crate) full_images: HashMap<PathBuf, egui::TextureHandle>,
    pub(crate) full_keys: VecDeque<PathBuf>,
    pub(crate) selected_indices: BTreeSet<usize>,
    pub(crate) selection_anchor: Option<usize>,
    pub(crate) selection_focus: Option<usize>,
    pub(crate) current_page: usize,
    pub(crate) presence_threshold: f32,
    pub(crate) pending_presence_threshold: f32,
    pub(crate) batch_size: usize,
    pub(crate) background_labels_input: String,
    pub(crate) background_labels: Vec<String>,
    pub(crate) preview: Option<PreviewState>,
    pub(crate) label_options: Vec<LabelOption>,
    pub(crate) new_label_buffer: String,
    pub(crate) export_present: bool,
    pub(crate) export_uncertain: bool,
    pub(crate) export_background: bool,
    pub(crate) export_csv: bool,
    pub(crate) pending_export: Option<PendingExport>,
    pub(crate) coordinate_prompt: Option<CoordinatePrompt>,
    pub(crate) manifest_status: ManifestStatus,
    pub(crate) update_rx: Option<Receiver<Result<crate::manifest::RemoteManifest, String>>>,
    pub(crate) model_download_status: ModelDownloadStatus,
    pub(crate) model_download_rx: Option<Receiver<Result<String, String>>>,
    pub(crate) app_version: String,
    pub(crate) model_version: String,
    pub(crate) model_root: PathBuf,
    pub(crate) improve_recognition: bool,
    pub(crate) roboflow_dataset_input: String,
    pub(crate) upload_status_tx: Sender<String>,
    pub(crate) upload_status_rx: Receiver<String>,
}

impl UiApp {
    /// Creates a new UI instance and kicks off the first manifest refresh.
    pub(crate) fn new() -> Self {
        let mut app = Self::default_internal();
        app.request_manifest_refresh();
        app
    }

    /// Internal constructor that wires all state defaults together.
    fn default_internal() -> Self {
        let (model_root, model_version) = Self::prepare_model_dir();
        let label_options = Self::load_label_options_from(&model_root.join("feeder-labels.csv"));
        let (upload_status_tx, upload_status_rx) = std::sync::mpsc::channel();
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
            selection_focus: None,
            current_page: 0,
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
            app_version: env!("FEEDIE_VERSION").to_string(),
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

/// Width/height of loaded thumbnails.
pub(crate) const THUMB_SIZE: u32 = 120;
/// Maximum number of thumbnails cached in memory at once.
pub(crate) const MAX_THUMBS: usize = 256;
/// Maximum number of full resolution textures cached for the preview window.
pub(crate) const MAX_FULL_IMAGES: usize = 32;
/// Hard limit to avoid decoding too many thumbnails per frame.
pub(crate) const MAX_THUMB_LOAD_PER_FRAME: usize = 12;
/// Width allocated for a thumbnail card.
pub(crate) const CARD_WIDTH: f32 = THUMB_SIZE as f32 + 40.0;
/// Height allocated for a thumbnail card.
pub(crate) const CARD_HEIGHT: f32 = THUMB_SIZE as f32 + 70.0;
/// Number of thumbnails displayed per page in the gallery view.
pub(crate) const PAGE_SIZE: usize = 100;
/// Built-in Roboflow API key for optional uploads.
pub(crate) const ROBOFLOW_API_KEY: &str = "g9zfZxZVNuSr43ENZJMg";
/// Remote manifest location that describes available updates.
pub(crate) const MANIFEST_URL: &str = "https://github.com/kpauly/feedie/raw/main/manifest.json";
/// Name of the bundled EfficientViT model weights.
pub(crate) const MODEL_FILE_NAME: &str = "feeder-efficientvit-m0.safetensors";
/// Name of the CSV file containing labels.
pub(crate) const LABEL_FILE_NAME: &str = "feeder-labels.csv";
/// Name of the on-disk file that stores the model version that was installed.
pub(crate) const VERSION_FILE_NAME: &str = "model_version.txt";

/// Messages that flow back from the background scanning thread.
///
/// `ScanMsg` keeps the UI decoupled from the worker and allows us to report
/// progress or replace the entire result set once classification finishes.
pub(crate) enum ScanMsg {
    Progress(usize, usize),
    Done(Vec<ImageInfo>, u128),
    Error(String),
}

impl App for UiApp {
    /// Called every egui frame to keep background tasks and panels responsive.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.refresh_background_state(ctx);
        self.render_navigation(ctx);
        self.render_active_panel(ctx);
        self.render_overlays(ctx);
        self.render_status_bar(ctx);
    }
}
