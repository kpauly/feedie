//! Navigation and frame orchestration helpers.

use super::{Panel, ScanMsg, UiApp};
use eframe::egui;
use std::time::Duration;

impl UiApp {
    /// Processes background channels and keeps long-running tasks responsive.
    pub(super) fn refresh_background_state(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.upload_status_rx.try_recv() {
            self.status = msg;
        }
        self.poll_manifest_updates();
        self.poll_model_download();
        self.poll_thumbnail_results(ctx);
        self.drain_scan_channel();
        if self.scan_in_progress || self.rx.is_some() || !self.thumb_inflight.is_empty() {
            ctx.request_repaint();
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    /// Renders the navigation bar that switches between panels.
    pub(super) fn render_navigation(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(self.tr("Fotomap", "Photo folder"))
                            .selected(self.panel == Panel::Folder),
                    )
                    .clicked()
                {
                    self.panel = Panel::Folder;
                }
                let can_view_results = self.has_scanned || self.scan_in_progress;
                if ui
                    .add_enabled(
                        can_view_results,
                        egui::Button::new(self.tr("Scanresultaat", "Results"))
                            .selected(self.panel == Panel::Results),
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
                        egui::Button::new(self.tr("Exporteren", "Export"))
                            .selected(self.panel == Panel::Export),
                    )
                    .clicked()
                {
                    self.panel = Panel::Export;
                }
                if ui
                    .add(
                        egui::Button::new(self.tr("Instellingen", "Settings"))
                            .selected(self.panel == Panel::Settings),
                    )
                    .clicked()
                {
                    self.panel = Panel::Settings;
                }
            });
        });
    }

    /// Draws whichever central panel is currently active.
    pub(super) fn render_active_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| match self.panel {
            Panel::Folder => self.render_folder_panel(ui, ctx),
            Panel::Results => self.render_results_panel(ui, ctx),
            Panel::Export => self.render_export_panel(ui),
            Panel::Settings => {
                egui::ScrollArea::vertical().show(ui, |ui| self.render_settings_panel(ui));
            }
        });
    }

    /// Renders windows that float above the panels.
    pub(super) fn render_overlays(&mut self, ctx: &egui::Context) {
        self.render_preview_window(ctx);
        self.render_coordinate_prompt(ctx);
    }

    /// Displays the persistent status bar at the bottom.
    pub(super) fn render_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status-bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.status_message());
            });
        });
    }

    /// Summarizes the current status string or returns a default.
    fn status_message(&self) -> String {
        if self.status.is_empty() {
            if self.scan_in_progress {
                self.tr("Bezig met scannen...", "Scanning...").to_string()
            } else if self.has_scanned {
                self.tr("Gereed.", "Done.").to_string()
            } else {
                self.tr("Klaar.", "Ready.").to_string()
            }
        } else {
            self.status.clone()
        }
    }

    /// Pulls messages from the scan worker and updates progress/result state.
    fn drain_scan_channel(&mut self) {
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
                        self.current_page = 0;
                        self.reset_thumbnail_cache();
                        self.presence_threshold = self.pending_presence_threshold;
                        self.apply_presence_threshold();
                        self.reset_selection();
                        self.save_cache_for_current_folder();
                        let totaal = self.total_files;
                        let (count_present, _, _) = self.view_counts();
                        self.status = match self.language {
                            crate::i18n::Language::Dutch => format!(
                                "Gereed: Dieren gevonden in {count_present} van {totaal} frames ({:.1} s)",
                                (elapsed_ms as f32) / 1000.0
                            ),
                            crate::i18n::Language::English => format!(
                                "Done: animals found in {count_present} of {totaal} frames ({:.1} s)",
                                (elapsed_ms as f32) / 1000.0
                            ),
                        };
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
    }
}
