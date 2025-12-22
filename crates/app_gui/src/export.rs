//! Export workflow for saving selections and CSV data.

use crate::app::{LabelOption, ROBOFLOW_API_KEY, UiApp};
use crate::roboflow::upload_to_roboflow;
use crate::util::{
    canonical_label, extract_timestamp, fallback_display_label, next_available_export_path,
    parse_coordinates, sanitize_for_path,
};
use anyhow::Context;
use arboard::Clipboard;
use chrono::{DateTime, Local};
use eframe::egui;
use feeder_core::{Classification, Decision, ImageInfo};
use rfd::FileDialog;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Controls which subsets of photos will be exported.
#[derive(Clone)]
/// User-facing export toggles expanded into actionable options.
struct ExportOptions {
    include_present: bool,
    include_uncertain: bool,
    include_background: bool,
    include_csv: bool,
}

/// Represents an export that still requires user input before it can run.
#[derive(Clone)]
pub(crate) struct PendingExport {
    target_dir: PathBuf,
    options: ExportOptions,
}

/// Summary that is shown to the user after an export finishes.
#[derive(Clone)]
struct ExportOutcome {
    copied: usize,
    wrote_csv: bool,
    target_dir: PathBuf,
}

/// Captures the work that needs to be done during an export run.
/// Unit of work for copying a single photo and optionally annotating it.
struct ExportJob {
    source: PathBuf,
    folder_label: String,
    canonical_label: Option<String>,
    include_in_csv: bool,
}

/// Form state for the CSV coordinate prompt.
#[derive(Default)]
pub(crate) struct CoordinatePrompt {
    pub(crate) input: String,
    pub(crate) error: Option<String>,
}

/// CSV record that mirrors a single exported observation.
/// In-memory representation of a CSV row.
struct CsvRecord {
    date: String,
    time: String,
    scientific: String,
    path: String,
}

impl UiApp {
    /// Renders the export options pane and action button.
    pub(crate) fn render_export_panel(&mut self, ui: &mut egui::Ui) {
        if !self.has_scanned || self.rijen.is_empty() {
            ui.label(self.tr(
                "Er zijn nog geen scanresultaten om te exporteren.",
                "There are no scan results to export yet.",
            ));
            return;
        }

        ui.heading(self.tr("Opties", "Options"));
        ui.add_space(4.0);
        let present_label = self.tr(
            "Exporteer foto's met aanwezige soorten",
            "Export photos with present species",
        );
        let uncertain_label = self.tr(
            "Exporteer foto's met onzekere identificatie",
            "Export photos with uncertain identification",
        );
        let background_label = self.tr(
            "Exporteer foto's uit Leeg (achtergrond)",
            "Export photos from Empty (background)",
        );
        let csv_label = self.tr(
            "Exporteer identificatieresultaten als CSV bestand",
            "Export identification results as CSV",
        );
        ui.checkbox(&mut self.export_present, present_label);
        ui.checkbox(&mut self.export_uncertain, uncertain_label);
        ui.checkbox(&mut self.export_background, background_label);
        let csv_checkbox = ui.checkbox(&mut self.export_csv, csv_label);
        if csv_checkbox.clicked() && self.export_csv {
            self.export_present = true;
        }

        ui.add_space(12.0);
        let can_export = self.can_export_from_panel();
        let button = ui.add_enabled(
            can_export,
            egui::Button::new(self.tr("Exporteer", "Export")),
        );
        if button.clicked() {
            self.start_export_workflow();
        }
        if !can_export {
            ui.label(self.tr(
                "Selecteer minstens een categorie om te exporteren.",
                "Select at least one category to export.",
            ));
        }
    }

    /// Determines whether the export button should be enabled.
    /// Returns true when at least one category is selected for export.
    fn can_export_from_panel(&self) -> bool {
        self.has_scanned
            && !self.rijen.is_empty()
            && (self.export_present || self.export_uncertain || self.export_background)
    }

    /// Opens the folder picker and prepares a pending export job.
    /// Opens the directory picker and stores the pending export configuration.
    fn start_export_workflow(&mut self) {
        if !self.can_export_from_panel() {
            self.status = self
                .tr("Geen foto's om te exporteren.", "No photos to export.")
                .to_string();
            return;
        }
        if self.export_csv && !self.export_present {
            self.status = self
                .tr(
                    "CSV export vereist dat 'aanwezige soorten' wordt meegekopieerd.",
                    "CSV export requires including present species.",
                )
                .to_string();
            return;
        }

        let mut dialog = FileDialog::new();
        if let Some(dir) = &self.gekozen_map {
            dialog = dialog.set_directory(dir);
        }
        let Some(target_dir) = dialog.pick_folder() else {
            self.status = self
                .tr("Export geannuleerd.", "Export cancelled.")
                .to_string();
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

    /// Shows a status message with the result of an export job.
    /// Displays feedback after the export job has finished.
    fn handle_export_result(&mut self, result: anyhow::Result<ExportOutcome>) {
        match result {
            Ok(summary) => {
                let mut message = if summary.copied == 0 {
                    format!(
                        "{} {}",
                        self.tr("Geen bestanden geexporteerd in", "No files exported to"),
                        summary.target_dir.display()
                    )
                } else {
                    format!(
                        "{} {} {}",
                        summary.copied,
                        self.tr("foto('s) geexporteerd naar", "photo(s) exported to"),
                        summary.target_dir.display()
                    )
                };
                if summary.wrote_csv {
                    message.push_str(self.tr("; CSV opgeslagen.", "; CSV saved."));
                }
                self.status = message;
            }
            Err(err) => {
                self.status = format!("{}: {err}", self.tr("Exporteren mislukt", "Export failed"));
            }
        }
    }

    /// Finalizes an export once the user provided coordinates.
    /// Continues the export containing CSV data once GPS coordinates are provided.
    fn complete_pending_export(&mut self, coords: (f64, f64)) {
        if let Some(pending) = self.pending_export.take() {
            let result = self.perform_export(pending, Some(coords));
            self.handle_export_result(result);
        }
        self.coordinate_prompt = None;
    }

    /// Collects GPS coordinates from the user before writing CSV exports.
    pub(crate) fn render_coordinate_prompt(&mut self, ctx: &egui::Context) {
        if self.coordinate_prompt.is_none() {
            return;
        }
        ctx.request_repaint();

        let mut close_requested = false;
        let mut submit_coords: Option<(f64, f64)> = None;
        let language = self.language;

        {
            let prompt = self.coordinate_prompt.as_mut().unwrap();
            let mut open = true;
            let title =
                crate::i18n::tr_for(language, "Coordinaten voor CSV", "Coordinates for CSV");
            let prompt_label = crate::i18n::tr_for(
                language,
                "Plak hier de Google Maps coordinaten:",
                "Paste Google Maps coordinates here:",
            );
            let paste_label = crate::i18n::tr_for(language, "Plakken", "Paste");
            let paste_failed = crate::i18n::tr_for(language, "Plakken mislukt", "Paste failed");
            let tip_intro = crate::i18n::tr_for(language, "Tip: open ", "Tip: open ");
            let tip_body = crate::i18n::tr_for(
                language,
                " en klik met de rechtermuisknop op de plaats van de camera. Klik vervolgens op de coordinaten bovenaan het verschenen keuzemenu. Deze worden automatisch naar het klembord gekopieerd. Klik opnieuw met de rechtermuisknop in het veld hierboven en kies plakken.",
                " and right-click the camera location. Then click the coordinates shown in the menu; they are copied to the clipboard. Right-click the field above and choose paste.",
            );
            let cancel_label = crate::i18n::tr_for(language, "Annuleer", "Cancel");
            let save_label = crate::i18n::tr_for(language, "Opslaan", "Save");
            let format_error = crate::i18n::tr_for(
                language,
                "Gebruik het formaat '<lat>, <lng>'.",
                "Use the format '<lat>, <lng>'.",
            );
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(prompt_label);
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut prompt.input)
                            .desired_width(260.0)
                            .hint_text("51.376318769269716, 4.456974517090091"),
                    );
                    response.context_menu(|ui| {
                        if ui.button(paste_label).clicked() {
                            match Clipboard::new().and_then(|mut cb| cb.get_text()) {
                                Ok(text) => {
                                    prompt.input = text;
                                    prompt.error = None;
                                }
                                Err(err) => {
                                    prompt.error = Some(format!("{}: {err}", paste_failed));
                                }
                            }
                            ui.close();
                        }
                    });

                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tip_intro);
                        ui.hyperlink_to("Google Maps", "https://maps.google.com");
                        ui.label(tip_body);
                    });

                    if let Some(err) = &prompt.error {
                        ui.add_space(4.0);
                        ui.colored_label(egui::Color32::RED, err);
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(cancel_label).clicked() {
                            close_requested = true;
                        }
                        let mut submit = false;
                        if ui.button(save_label).clicked() {
                            submit = true;
                        }
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            submit = true;
                        }
                        if submit {
                            match parse_coordinates(&prompt.input) {
                                Ok(coords) => {
                                    submit_coords = Some(coords);
                                }
                                Err(_) => {
                                    prompt.error = Some(format_error.to_string());
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
            self.status = self
                .tr("Export geannuleerd.", "Export cancelled.")
                .to_string();
        }
    }

    /// Copies the currently selected thumbnails into a destination folder.
    pub(crate) fn export_selected_images(&mut self, indices: &[usize]) {
        if indices.is_empty() {
            self.status = self
                .tr(
                    "Geen foto's geselecteerd voor export.",
                    "No photos selected for export.",
                )
                .to_string();
            return;
        }

        let mut dialog = FileDialog::new();
        if let Some(dir) = &self.gekozen_map {
            dialog = dialog.set_directory(dir);
        }

        let Some(target_dir) = dialog.pick_folder() else {
            self.status = self
                .tr("Export geannuleerd.", "Export cancelled.")
                .to_string();
            return;
        };

        match self.copy_selection_to(&target_dir, indices) {
            Ok(0) => {
                self.status = self
                    .tr(
                        "Geen export uitgevoerd: geen bruikbare bestanden gevonden.",
                        "No export performed: no usable files found.",
                    )
                    .to_string();
            }
            Ok(count) => {
                self.status = format!(
                    "{} {} {}",
                    count,
                    self.tr("foto('s) geexporteerd naar", "photo(s) exported to"),
                    target_dir.display()
                );
            }
            Err(err) => {
                self.status = format!("{}: {err}", self.tr("Exporteren mislukt", "Export failed"));
            }
        }
    }

    /// Copies the underlying files for the supplied indices into `target_dir`.
    /// Copies the requested files to the export directory and returns the count.
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
            fs::create_dir_all(&label_dir).with_context(|| {
                format!(
                    "{} {}",
                    self.tr("Kon map niet aanmaken", "Could not create folder"),
                    label_dir.display()
                )
            })?;

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
                    "{} {} -> {}",
                    self.tr("Kopieren mislukt", "Copy failed"),
                    info.file.display(),
                    dest_path.display()
                )
            })?;
            copied += 1;
        }

        Ok(copied)
    }

    /// Picks the best label to use when exporting the provided image.
    /// Chooses the best display label for the export folder name.
    fn label_for_export(&self, info: &ImageInfo) -> String {
        match info.classification.as_ref().map(|c| &c.decision) {
            Some(Decision::Label(name)) => self.display_for(name),
            Some(Decision::Unknown) => self.tr("Onbekend", "Unknown").to_string(),
            None => {
                if info.present {
                    self.tr("Onbekend", "Unknown").to_string()
                } else {
                    self.tr("Leeg", "Empty").to_string()
                }
            }
        }
    }

    /// Returns the set of labels that can be manually applied.
    pub(crate) fn available_labels(&self) -> Vec<String> {
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

    /// Performs the export workflow and optionally emits a CSV.
    /// Executes the configured export and optionally writes the CSV summary.
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
            return Err(anyhow!(self.tr(
                "Coordinaten ontbreken voor CSV-export",
                "Coordinates missing for CSV export",
            )));
        }

        let jobs = self.collect_export_jobs(&options);
        if jobs.is_empty() && !options.include_csv {
            return Err(anyhow!(self.tr(
                "Geen bestanden voldeden aan de huidige selectie.",
                "No files matched the current selection.",
            )));
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
            fs::create_dir_all(&folder_path).with_context(|| {
                format!(
                    "{} {}",
                    self.tr("Kon map niet aanmaken", "Could not create folder"),
                    folder_path.display()
                )
            })?;

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
                    "{} {} -> {}",
                    self.tr("Kopieren mislukt", "Copy failed"),
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
            write_export_csv(
                &target_dir,
                &csv_records,
                coords,
                export_time,
                self.language,
            )?;
        }

        Ok(ExportOutcome {
            copied,
            wrote_csv: options.include_csv,
            target_dir,
        })
    }
    /// Collects all export jobs that match the configured options.
    /// Builds the list of items that should be exported for the selected options.
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
                    folder_label: self.tr("Onzeker", "Uncertain").to_string(),
                    canonical_label: None,
                    include_in_csv: false,
                });
            }
            if options.include_background && self.belongs_in_leeg(info) {
                jobs.push(ExportJob {
                    source: info.file.clone(),
                    folder_label: self.tr("Leeg", "Empty").to_string(),
                    canonical_label: None,
                    include_in_csv: false,
                });
            }
        }
        jobs
    }

    /// Resolves the display and canonical label for present detections.
    /// Returns the canonical/display label for rows considered present.
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

    /// Determines whether a capture should be treated as background.
    /// Returns true if a row falls under the "Leeg" export bucket.
    fn belongs_in_leeg(&self, info: &ImageInfo) -> bool {
        !info.present && !self.is_onzeker(info)
    }

    /// Looks up the scientific name for a canonical label if known.
    /// Finds the optional scientific name for the provided canonical label.
    fn scientific_for(&self, canonical: &str) -> Option<String> {
        self.label_options
            .iter()
            .find(|option| option.canonical == canonical)
            .and_then(|option| option.scientific.clone())
    }

    /// Applies a manual label assignment to the selected rows.
    pub(crate) fn assign_manual_category(
        &mut self,
        indices: &[usize],
        label: String,
        mark_present: bool,
    ) {
        let canonical = canonical_label(&label);
        let display = self.display_for(&canonical);
        let manual_label = format!("{canonical} (manueel)");
        let mut paths: Vec<PathBuf> = Vec::new();
        for &idx in indices {
            if let Some(info) = self.rijen.get_mut(idx) {
                info.classification = Some(Classification {
                    decision: Decision::Label(manual_label.clone()),
                    confidence: 1.0,
                });
                info.present = mark_present && canonical != "achtergrond";
                paths.push(info.file.clone());
            }
        }
        self.status = format!(
            "{} {} {}",
            indices.len(),
            self.tr("kaart(en) gemarkeerd als", "item(s) marked as"),
            display
        );

        // Background upload to Roboflow if enabled and configured
        if self.improve_recognition {
            let dataset = self
                .roboflow_dataset_input
                .trim()
                .trim_matches('/')
                .to_string();
            let label_for_upload = canonical.clone();
            let api_key = ROBOFLOW_API_KEY.trim();
            if api_key.is_empty() {
                self.status = self
                    .tr(
                        "Roboflow upload staat aan, maar er is geen API-sleutel ingebouwd.",
                        "Roboflow upload is enabled, but no API key is embedded.",
                    )
                    .to_string();
            } else if dataset.is_empty() {
                self.status = self
                    .tr(
                        "Roboflow upload niet uitgevoerd: dataset ontbreekt.",
                        "Roboflow upload skipped: dataset missing.",
                    )
                    .to_string();
            } else if paths.is_empty() {
                self.status = self
                    .tr(
                        "Roboflow upload niet uitgevoerd: geen foto's geselecteerd.",
                        "Roboflow upload skipped: no photos selected.",
                    )
                    .to_string();
            } else {
                let upload_count = paths.len();
                let status_tx = self.upload_status_tx.clone();
                let language = self.language;
                self.status = self
                    .tr(
                        "Foto('s) met manuele identificatie worden geupload...",
                        "Photo(s) with manual labels are being uploaded...",
                    )
                    .to_string();
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
                        format!(
                            "{}: {err}",
                            crate::i18n::tr_for(
                                language,
                                "Upload van foto('s) met manuele identificatie mislukt",
                                "Upload of photos with manual labels failed",
                            )
                        )
                    } else if upload_count == 1 {
                        crate::i18n::tr_for(
                            language,
                            "Foto met manuele identificatie geupload.",
                            "Photo with manual label uploaded.",
                        )
                        .to_string()
                    } else {
                        format!(
                            "{} {}",
                            upload_count,
                            crate::i18n::tr_for(
                                language,
                                "foto's met manuele identificatie geupload.",
                                "photos with manual labels uploaded.",
                            )
                        )
                    };
                    let _ = status_tx.send(message);
                });
            }
        }

        // Persist updated labels to cache if possible
        self.save_cache_for_current_folder();
    }
    /// Adds a new manual label selected by the user.
    pub(crate) fn apply_new_label(&mut self, indices: &[usize]) -> bool {
        let trimmed = self.new_label_buffer.trim();
        if trimmed.is_empty() {
            self.status = self
                .tr("Geen label ingevuld.", "No label entered.")
                .to_string();
            return false;
        }
        let new_label = trimmed.to_string();
        let canonical = canonical_label(&new_label);
        if canonical.is_empty() {
            self.status = self
                .tr("Label is ongeldig.", "Label is invalid.")
                .to_string();
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
                display_en: None,
                scientific: None,
            });
        }
        self.assign_manual_category(indices, new_label, true);
        self.new_label_buffer.clear();
        true
    }

    /// Returns the indices that should be affected by a context menu action.
    pub(crate) fn context_targets(&self, idx: usize) -> Vec<usize> {
        if self.selected_indices.contains(&idx) && !self.selected_indices.is_empty() {
            self.selected_indices.iter().copied().collect()
        } else {
            vec![idx]
        }
    }

    /// Returns the localized display label for the provided canonical name.
    pub(crate) fn display_for(&self, name: &str) -> String {
        let canonical = canonical_label(name);
        if canonical == "iets sp" {
            return self.tr("Iets sp.", "Something sp.").to_string();
        }
        if canonical == "achtergrond" {
            return self.tr("Achtergrond", "Background").to_string();
        }
        if let Some(option) = self
            .label_options
            .iter()
            .find(|option| option.canonical == canonical)
        {
            if matches!(self.language, crate::i18n::Language::English)
                && let Some(display_en) = &option.display_en
            {
                return display_en.clone();
            }
            return option.display.clone();
        }
        fallback_display_label(&canonical)
    }
}

/// Writes the CSV summary file for a completed export.
/// Writes the CSV summary that Roboflow/others can ingest.
fn write_export_csv(
    dir: &Path,
    records: &[CsvRecord],
    coords: (f64, f64),
    export_time: DateTime<Local>,
    language: crate::i18n::Language,
) -> anyhow::Result<PathBuf> {
    let base = format!("voederhuiscamera_{}", export_time.format("%y%m%d%H%M"));
    let csv_path = next_available_export_path(dir, &base, "csv");
    let open_error = crate::i18n::tr_for(
        language,
        "Kon CSV-bestand niet openen",
        "Could not open CSV file",
    );
    let mut writer = csv::Writer::from_path(&csv_path)
        .with_context(|| format!("{} {}", open_error, csv_path.display()))?;
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
