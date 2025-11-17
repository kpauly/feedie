//! Settings panel rendering for thresholds, uploads, and updates.

use super::{Panel, UiApp};
use eframe::egui;

impl UiApp {
    /// Renders the settings screen including thresholds and telemetry toggles.
    pub(super) fn render_settings_panel(&mut self, ui: &mut egui::Ui) {
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
}
