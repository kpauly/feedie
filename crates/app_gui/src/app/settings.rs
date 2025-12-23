//! Settings panel rendering for thresholds, uploads, and updates.

use super::{BACKGROUND_LABEL, Panel, SOMETHING_LABEL, UiApp};
use crate::i18n::LanguagePreference;
use eframe::egui;

impl UiApp {
    /// Renders the settings screen including thresholds and telemetry toggles.
    pub(super) fn render_settings_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.t("settings-title"));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(self.t("settings-language"));
            let mut selected = self.language_preference;
            let system_language = crate::i18n::detect_system_language();
            let system_label = crate::i18n::t_for(system_language, "language-option-system");
            let option_label = |lang: LanguagePreference| -> String {
                match lang {
                    LanguagePreference::System => system_label.clone(),
                    LanguagePreference::Dutch => "Nederlands".to_string(),
                    LanguagePreference::English => "English".to_string(),
                    LanguagePreference::French => "Français".to_string(),
                    LanguagePreference::German => "Deutsch".to_string(),
                    LanguagePreference::Spanish => "Español".to_string(),
                    LanguagePreference::Swedish => "Svenska".to_string(),
                }
            };
            egui::ComboBox::from_id_salt("language-select")
                .selected_text(option_label(selected))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut selected,
                        LanguagePreference::System,
                        option_label(LanguagePreference::System),
                    );
                    ui.selectable_value(
                        &mut selected,
                        LanguagePreference::Dutch,
                        option_label(LanguagePreference::Dutch),
                    );
                    ui.selectable_value(
                        &mut selected,
                        LanguagePreference::English,
                        option_label(LanguagePreference::English),
                    );
                    ui.selectable_value(
                        &mut selected,
                        LanguagePreference::French,
                        option_label(LanguagePreference::French),
                    );
                    ui.selectable_value(
                        &mut selected,
                        LanguagePreference::German,
                        option_label(LanguagePreference::German),
                    );
                    ui.selectable_value(
                        &mut selected,
                        LanguagePreference::Spanish,
                        option_label(LanguagePreference::Spanish),
                    );
                    ui.selectable_value(
                        &mut selected,
                        LanguagePreference::Swedish,
                        option_label(LanguagePreference::Swedish),
                    );
                });
            if selected != self.language_preference {
                self.update_language_preference(selected);
                self.status = self.t("settings-language-updated");
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let threshold_label = self.t("settings-uncertainty-threshold");
            let slider = egui::Slider::new(&mut self.pending_presence_threshold, 0.0..=1.0)
                .text(threshold_label)
                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0));
            ui.add(slider);
            if ui.button(self.t("action-recompute")).clicked() {
                self.presence_threshold = self.pending_presence_threshold;
                self.apply_presence_threshold();
                self.status = format!(
                    "{}: {:.0}%",
                    self.t("settings-threshold-applied"),
                    self.presence_threshold * 100.0
                );
                self.panel = Panel::Results;
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(self.t("settings-background-labels"));
            let background_label = self.display_for(BACKGROUND_LABEL);
            let something_label = self.display_for(SOMETHING_LABEL);
            let mut include_something = self
                .background_labels
                .iter()
                .any(|label| label == SOMETHING_LABEL);
            let selected_text = if include_something {
                format!("{background_label}, {something_label}")
            } else {
                background_label.clone()
            };
            let background_label_ui = background_label.clone();
            let something_label_ui = something_label.clone();
            egui::ComboBox::from_id_salt("background-labels")
                .selected_text(selected_text)
                .show_ui(ui, |ui| {
                    let mut background_selected = self
                        .background_labels
                        .iter()
                        .any(|label| label == BACKGROUND_LABEL);
                    ui.add_enabled(
                        false,
                        egui::Checkbox::new(&mut background_selected, background_label_ui),
                    );
                    ui.checkbox(&mut include_something, something_label_ui);
                });
            let mut updated = vec![BACKGROUND_LABEL.to_string()];
            if include_something {
                updated.push(SOMETHING_LABEL.to_string());
            }
            if updated != self.background_labels {
                self.update_background_labels(updated);
                self.status = self.t("settings-background-updated");
            }
        });

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(6.0);
        let improve_label = self.t("settings-improve-recognition");
        ui.checkbox(&mut self.improve_recognition, improve_label);
        ui.label(self.t("settings-improve-help"));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(self.t("settings-roboflow-dataset"));
            ui.text_edit_singleline(&mut self.roboflow_dataset_input);
        });
        ui.add_space(4.0);
        ui.label(self.t("settings-roboflow-note"));

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(6.0);
        ui.heading(self.t("settings-versions"));
        ui.label(format!(
            "{}: {}",
            self.t("settings-app-version"),
            self.app_version
        ));
        ui.label(format!(
            "{}: {}",
            self.t("settings-model-version"),
            self.model_version
        ));
        self.render_update_section(ui);
    }
}
