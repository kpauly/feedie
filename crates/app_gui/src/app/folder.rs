use super::{Panel, ScanMsg, UiApp, ViewMode};
use eframe::egui;
use feeder_core::{EfficientVitClassifier, ScanOptions, scan_folder_with};
use rfd::FileDialog;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Instant;

impl UiApp {
    /// Displays the folder selection UI and scan controls.
    pub(super) fn render_folder_panel(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
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

    /// Shows a compact progress indicator while a scan is running.
    pub(super) fn render_progress_ui(&self, ui: &mut egui::Ui) {
        let total = self.total_files.max(1);
        let frac = (self.scanned_count as f32) / (total as f32);
        ui.add(egui::ProgressBar::new(frac).text(format!(
            "Scannen... {} / {} ({:.0}%)",
            self.scanned_count,
            self.total_files,
            frac * 100.0
        )));
    }

    /// Updates state when the user chose a new folder to scan.
    pub(super) fn set_selected_folder(&mut self, dir: PathBuf) {
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

    /// Kicks off an asynchronous scan job for the selected folder.
    pub(super) fn start_scan(&mut self, dir: PathBuf) {
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
}
