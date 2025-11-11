use eframe::{App, Frame, NativeOptions, egui};
use feeder_core::{
    ClassifierConfig, EfficientNetClassifier, ImageInfo, ScanOptions, export_csv, scan_folder_with,
};
use rfd::FileDialog;
use std::collections::{HashMap, VecDeque};
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
}

#[derive(Default)]
struct UiApp {
    gekozen_map: Option<PathBuf>,
    // Full results for the selected folder (after scanning)
    rijen: Vec<ImageInfo>,
    // Pre-scan info and scan status
    total_files: usize,
    scanned_count: usize,
    has_scanned: bool,
    scan_in_progress: bool,
    status: String,
    view: ViewMode,
    // Background scan channel
    rx: Option<Receiver<ScanMsg>>,
    // Thumbnail cache (basic LRU)
    thumbs: HashMap<PathBuf, egui::TextureHandle>,
    thumb_keys: VecDeque<PathBuf>,
}

const THUMB_SIZE: u32 = 120;
const MAX_THUMBS: usize = 256;

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
                        let totaal = self.total_files;
                        let aanwezig = self.rijen.iter().filter(|r| r.present).count();
                        self.status = format!(
                            "Gereed: Dieren gevonden in {aanwezig} van {totaal} frames ({:.1} s)",
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
                    .add_enabled(!self.scan_in_progress, egui::Button::new("Kies map..."))
                    .clicked()
                    && let Some(dir) = FileDialog::new().set_directory(".").pick_folder()
                {
                    self.gekozen_map = Some(dir.clone());
                    self.rijen.clear();
                    self.status.clear();
                    self.has_scanned = false;
                    self.scanned_count = 0;
                    self.total_files = 0;
                    self.view = ViewMode::Aanwezig;
                    self.thumbs.clear();
                    self.thumb_keys.clear();
                    // Pre-scan: count supported images (non-recursive for v0)
                    match scan_folder_with(&dir, ScanOptions { recursive: false }) {
                        Ok(rows) => {
                            self.total_files = rows.len();
                        }
                        Err(e) => {
                            self.status = format!("Fout bij lezen van map: {e}");
                        }
                    }
                }

                let kan_scannen = self.gekozen_map.is_some()
                    && !self.scan_in_progress
                    && !self.status.contains("Fout");
                if ui
                    .add_enabled(kan_scannen, egui::Button::new("Scannen"))
                    .clicked()
                    && let Some(dir) = self.gekozen_map.clone()
                {
                    self.scan_in_progress = true;
                    self.status = "Bezig met scannen...".to_string();
                    self.scanned_count = 0;
                    // Background worker
                    let (tx, rx): (Sender<ScanMsg>, Receiver<ScanMsg>) = mpsc::channel();
                    self.rx = Some(rx);
                    thread::spawn(move || {
                        let t0 = Instant::now();
                        let mut rows =
                            match scan_folder_with(&dir, ScanOptions { recursive: false }) {
                                Ok(r) => r,
                                Err(e) => {
                                    let _ = tx
                                        .send(ScanMsg::Error(format!("Map scannen mislukt: {e}")));
                                    tracing::warn!("scan_folder_with failed: {}", e);
                                    return;
                                }
                            };
                        let total = rows.len();
                        let _ = tx.send(ScanMsg::Progress(0, total));
                        let cfg = ClassifierConfig::default();
                        let classifier = match EfficientNetClassifier::new(&cfg) {
                            Ok(c) => c,
                            Err(e) => {
                                let _ =
                                    tx.send(ScanMsg::Error(format!("Model laden mislukt: {e}")));
                                return;
                            }
                        };
                        let tx_progress = tx.clone();
                        if let Err(e) =
                            classifier.classify_with_progress(&mut rows, |done, total| {
                                let _ = tx_progress.send(ScanMsg::Progress(done.min(total), total));
                            })
                        {
                            let _ = tx.send(ScanMsg::Error(format!("Classificatie mislukt: {e}")));
                            return;
                        }

                        let _ = tx.send(ScanMsg::Progress(total, total));
                        let elapsed_ms = t0.elapsed().as_millis();
                        let _ = tx.send(ScanMsg::Done(rows, elapsed_ms));
                    });
                }

                let kan_exporteren =
                    self.has_scanned && !self.rijen.is_empty() && !self.scan_in_progress;
                if ui
                    .add_enabled(kan_exporteren, egui::Button::new("Exporteer CSV"))
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

                if !self.status.is_empty() {
                    ui.label(&self.status);
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // Pre-scan summary
            if self.gekozen_map.is_some() && !self.has_scanned && !self.scan_in_progress {
                ui.label(format!("Afbeeldingen in map: {}", self.total_files));
                if self.total_files == 0 {
                    ui.heading("Geen afbeeldingen gevonden");
                }
            }

            // Progress during scanning
            if self.scan_in_progress {
                let total = self.total_files.max(1);
                let frac = (self.scanned_count as f32) / (total as f32);
                ui.add(egui::ProgressBar::new(frac).text(format!(
                    "Scannen... {} / {} ({:.0}%)",
                    self.scanned_count,
                    self.total_files,
                    frac * 100.0
                )));
                return; // Skip gallery while scanning
            }

            // Post-scan summary and gallery
            if self.has_scanned {
                let totaal = self.total_files;
                let aanwezig = self.rijen.iter().filter(|r| r.present).count();
                ui.label(format!("Dieren gevonden in {aanwezig} van {totaal} frames"));

                // View toggle
                ui.horizontal(|ui| {
                    let present_btn =
                        ui.selectable_label(self.view == ViewMode::Aanwezig, "Aanwezig");
                    let empty_btn = ui.selectable_label(self.view == ViewMode::Leeg, "Leeg");
                    if present_btn.clicked() {
                        self.view = ViewMode::Aanwezig;
                        self.thumbs.clear();
                        self.thumb_keys.clear();
                    }
                    if empty_btn.clicked() {
                        self.view = ViewMode::Leeg;
                        self.thumbs.clear();
                        self.thumb_keys.clear();
                    }
                });

                // Filter rows for current view
                let filtered: Vec<PathBuf> = match self.view {
                    ViewMode::Aanwezig => self
                        .rijen
                        .iter()
                        .filter(|r| r.present)
                        .map(|r| r.file.clone())
                        .collect(),
                    ViewMode::Leeg => self
                        .rijen
                        .iter()
                        .filter(|r| !r.present)
                        .map(|r| r.file.clone())
                        .collect(),
                };

                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let thumb_px = THUMB_SIZE as f32;
                            let desired = egui::Vec2::new(thumb_px, thumb_px);
                            let mut loaded_this_frame = 0usize;
                            const MAX_LOAD_PER_FRAME: usize = 12;

                            for path in filtered {
                                let (resp, painter) =
                                    ui.allocate_painter(desired, egui::Sense::hover());
                                let r = resp.rect;
                                let had_tex = self.thumbs.contains_key(&path);
                                if !had_tex && loaded_this_frame >= MAX_LOAD_PER_FRAME {
                                    painter.rect_filled(r, 4.0, egui::Color32::from_gray(40));
                                    painter.rect_stroke(
                                        r,
                                        4.0,
                                        egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                                        egui::StrokeKind::Inside,
                                    );
                                    continue;
                                }
                                if let Some(id) = self.get_or_load_thumb(ctx, &path) {
                                    if !had_tex {
                                        loaded_this_frame += 1;
                                    }
                                    let uv = egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    );
                                    painter.image(id, uv, r, egui::Color32::WHITE);
                                } else {
                                    painter.rect_filled(r, 4.0, egui::Color32::from_gray(40));
                                    painter.rect_stroke(
                                        r,
                                        4.0,
                                        egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                            }
                        });
                    });
            }
        });
    }
}
