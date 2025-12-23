//! Folder selection workflow and scan orchestration.

use super::{Panel, ScanMsg, UiApp, ViewMode};
use eframe::egui;
use feeder_core::{EfficientVitClassifier, ImageInfo, ScanOptions, scan_folder_with};
use rfd::FileDialog;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Instant;

impl UiApp {
    /// Displays the folder selection UI and scan controls.
    pub(super) fn render_folder_panel(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        if let Some(path) = &self.gekozen_map {
            ui.label(format!(
                "{}: {}",
                self.t("nav-photo-folder"),
                path.display()
            ));
            ui.label(format!(
                "{}: {}",
                self.t("folder-images-count"),
                self.total_files
            ));
        } else {
            ui.label(self.t("folder-no-selection"));
        }
        ui.add_space(8.0);
        if ui
            .add_enabled(
                !self.scan_in_progress,
                egui::Button::new(self.t("folder-choose")),
            )
            .clicked()
            && let Some(dir) = FileDialog::new().set_directory(".").pick_folder()
        {
            self.set_selected_folder(dir);
        }
        let can_scan = self.gekozen_map.is_some() && !self.scan_in_progress;
        if ui
            .add_enabled(can_scan, egui::Button::new(self.t("folder-scan")))
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
            "{}... {} / {} ({:.0}%)",
            self.t("scan-progress"),
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
        self.reset_selection();
        self.current_page = 0;
        self.reset_thumbnail_cache();
        self.full_images.clear();
        self.full_keys.clear();
        match scan_folder_with(&dir, ScanOptions { recursive: false }) {
            Ok(rows) => {
                self.total_files = rows.len();
                match self.try_load_cached_scan(&dir) {
                    Ok(true) => {
                        self.panel = Panel::Results;
                    }
                    Ok(false) => {}
                    Err(err) => {
                        tracing::warn!("Cache load failed: {err}");
                    }
                }
            }
            Err(e) => {
                self.status = format!("{}: {e}", self.t("folder-read-error"));
            }
        }
    }

    /// Kicks off an asynchronous scan job for the selected folder.
    pub(super) fn start_scan(&mut self, dir: PathBuf) {
        self.scan_in_progress = true;
        self.status = self.t("status-scanning");
        self.scanned_count = 0;
        self.panel = Panel::Results;
        let (tx, rx): (Sender<ScanMsg>, Receiver<ScanMsg>) = mpsc::channel();
        self.rx = Some(rx);
        let cfg = self.classifier_config();
        let language = self.language;
        thread::spawn(move || {
            let t0 = Instant::now();
            let mut rows = match scan_folder_with(&dir, ScanOptions { recursive: false }) {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(ScanMsg::Error(format!(
                        "{}: {e}",
                        crate::i18n::t_for(language, "scan-failed")
                    )));
                    tracing::warn!("scan_folder_with failed: {}", e);
                    return;
                }
            };
            let total = rows.len();
            let _ = tx.send(ScanMsg::Progress(0, total));
            let classifier = match EfficientVitClassifier::new(&cfg) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(ScanMsg::Error(format!(
                        "{}: {e}",
                        crate::i18n::t_for(language, "model-load-failed")
                    )));
                    return;
                }
            };
            let tx_progress = tx.clone();
            if let Err(e) = classify_with_auto_batch(&classifier, &mut rows, |done, total| {
                let _ = tx_progress.send(ScanMsg::Progress(done.min(total), total));
            }) {
                let _ = tx.send(ScanMsg::Error(format!(
                    "{}: {e}",
                    crate::i18n::t_for(language, "classification-failed")
                )));
                return;
            }

            let _ = tx.send(ScanMsg::Progress(total, total));
            let elapsed_ms = t0.elapsed().as_millis();
            let _ = tx.send(ScanMsg::Done(rows, elapsed_ms));
        });
    }
}

const AUTO_BATCH_MIN_TOTAL: usize = 1000;
const AUTO_BATCH_BASELINE: usize = 8;
const AUTO_BATCH_CANDIDATES: [usize; 2] = [AUTO_BATCH_BASELINE, 12];
const AUTO_BATCH_TUNE_BATCHES: usize = 4;
const AUTO_BATCH_MIN_IMPROVEMENT: f64 = 0.15;

fn classify_with_auto_batch<F>(
    classifier: &EfficientVitClassifier,
    rows: &mut [ImageInfo],
    mut progress: F,
) -> anyhow::Result<()>
where
    F: FnMut(usize, usize),
{
    let total = rows.len();
    if total == 0 {
        return Ok(());
    }
    if total < AUTO_BATCH_MIN_TOTAL {
        return classifier.classify_with_progress_and_batch_size(
            rows,
            AUTO_BATCH_BASELINE,
            progress,
        );
    }

    let mut offset = 0usize;
    let mut timings: Vec<(usize, f64)> = Vec::new();
    for &candidate in AUTO_BATCH_CANDIDATES.iter() {
        let tune_len = candidate * AUTO_BATCH_TUNE_BATCHES;
        if offset + tune_len > total {
            break;
        }
        let start = Instant::now();
        let mut local_done = 0usize;
        classifier.classify_with_progress_and_batch_size(
            &mut rows[offset..offset + tune_len],
            candidate,
            |done, _| {
                if done == local_done {
                    return;
                }
                local_done = done;
                progress(offset + local_done, total);
            },
        )?;
        let elapsed = start.elapsed().as_secs_f64();
        if local_done > 0 {
            timings.push((candidate, elapsed / local_done as f64));
        }
        offset += local_done;
    }

    let mut chosen = AUTO_BATCH_BASELINE;
    if let Some(base_time) = timings
        .iter()
        .find(|(size, _)| *size == AUTO_BATCH_BASELINE)
        .map(|(_, per_image)| *per_image)
    {
        let mut best_time = base_time;
        for (size, per_image) in &timings {
            if *size == AUTO_BATCH_BASELINE {
                continue;
            }
            if *per_image < base_time * (1.0 - AUTO_BATCH_MIN_IMPROVEMENT) && *per_image < best_time
            {
                best_time = *per_image;
                chosen = *size;
            }
        }
    } else if let Some((size, _)) = timings.first() {
        chosen = *size;
    } else {
        chosen = AUTO_BATCH_BASELINE;
    }

    if offset < total {
        classifier.classify_with_progress_and_batch_size(
            &mut rows[offset..],
            chosen,
            |done, _| {
                progress(offset + done, total);
            },
        )?;
    }
    Ok(())
}
