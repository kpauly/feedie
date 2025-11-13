use eframe::{App, Frame, NativeOptions, egui};
use feeder_core::{
    Classification, ClassifierConfig, Decision, EfficientVitClassifier, ImageInfo, ScanOptions,
    export_csv, scan_folder_with,
};
use rfd::FileDialog;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    tracing_subscriber::fmt::init();
    let options = NativeOptions::default();
    if let Err(e) = eframe::run_native(
        "Feeder Vision (preview)",
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
}

impl Default for UiApp {
    fn default() -> Self {
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
            label_options: Self::load_label_options(),
        }
    }
}

const THUMB_SIZE: u32 = 120;
const MAX_THUMBS: usize = 256;
const MAX_FULL_IMAGES: usize = 32;
const MAX_THUMB_LOAD_PER_FRAME: usize = 12;
const CARD_WIDTH: f32 = THUMB_SIZE as f32 + 40.0;
const CARD_HEIGHT: f32 = THUMB_SIZE as f32 + 70.0;

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

    fn thumbnail_caption(info: &ImageInfo) -> String {
        match &info.classification {
            Some(classification) => {
                let label = match &classification.decision {
                    Decision::Label(name) => {
                        if let Some(stripped) = name.strip_suffix(" (manueel)") {
                            return format!("{stripped} (manueel)");
                        }
                        name.clone()
                    }
                    Decision::Unknown => "Leeg".to_string(),
                };
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
            let caption = Self::thumbnail_caption(info);
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
        {
            if let Some(dir) = FileDialog::new().set_directory(".").pick_folder() {
                self.set_selected_folder(dir);
            }
        }
        let can_scan = self.gekozen_map.is_some() && !self.scan_in_progress;
        if ui
            .add_enabled(can_scan, egui::Button::new("Scannen"))
            .clicked()
        {
            if let Some(dir) = self.gekozen_map.clone() {
                self.start_scan(dir);
                self.panel = Panel::Results;
            }
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
        let mut cfg = ClassifierConfig::default();
        cfg.presence_threshold = self.pending_presence_threshold;
        cfg.batch_size = self.batch_size.max(1);
        cfg.background_labels = self.background_labels.clone();
        cfg
    }
}

impl App for UiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
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
                let can_export =
                    self.has_scanned && !self.rijen.is_empty() && !self.scan_in_progress;
                if ui
                    .add_enabled(can_export, egui::Button::new("Exporteren"))
                    .clicked()
                    && let Some(path) = FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .set_file_name("feeder_vision.csv")
                        .save_file()
                {
                    if let Err(e) = export_csv(&self.rijen, &path) {
                        self.status = format!("Fout bij exporteren: {e}");
                    } else {
                        self.status = format!("CSV opgeslagen: {}", path.display());
                    }
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
            Panel::Settings => self.render_settings_panel(ui),
        });

        self.render_preview_window(ctx);

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
    fn load_label_options() -> Vec<LabelOption> {
        let path = PathBuf::from("models/feeder-labels.csv");
        let Ok(content) = std::fs::read_to_string(&path) else {
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
            let canonical = canonical_label(trimmed);
            if canonical.is_empty() || !seen.insert(canonical.clone()) {
                continue;
            }
            options.push(LabelOption {
                canonical,
                display: trimmed.to_string(),
            });
        }
        options
    }

    fn render_context_menu(&mut self, ui: &mut egui::Ui, indices: &[usize]) {
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
            if let Some(classification) = &info.classification {
                if let Decision::Label(name) = &classification.decision {
                    let canonical = canonical_label(name);
                    if canonical == "achtergrond" || canonical == "iets sp" {
                        continue;
                    }
                    if seen.insert(canonical.clone()) {
                        ordered.push(canonical);
                    }
                }
            }
        }
        ordered
    }

    fn assign_manual_category(&mut self, indices: &[usize], label: String, mark_present: bool) {
        let lower = canonical_label(&label);
        let display = self.display_for(&lower);
        for &idx in indices {
            if let Some(info) = self.rijen.get_mut(idx) {
                info.classification = Some(Classification {
                    decision: Decision::Label(format!("{display} (manueel)")),
                    confidence: 1.0,
                });
                info.present = mark_present && lower != "achtergrond";
            }
        }
        self.status = format!("{} kaart(en) gemarkeerd als {}", indices.len(), display);
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
    let cleaned = stripped.trim_end_matches('.');
    cleaned.to_ascii_lowercase()
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
