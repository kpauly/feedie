//! Rendering of the results grid and associated interactions.

use super::{CARD_HEIGHT, CARD_WIDTH, MAX_THUMB_LOAD_PER_FRAME, THUMB_SIZE, UiApp, ViewMode};
use eframe::egui;
use feeder_core::{Decision, ImageInfo};

impl UiApp {
    /// Returns the caption that is shown under every thumbnail.
    pub(super) fn thumbnail_caption(&self, info: &ImageInfo) -> String {
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

    /// Draws a single thumbnail card within the result grid.
    pub(super) fn draw_thumbnail_card(
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

    /// Renders the panel that shows scan results and thumbnails.
    pub(super) fn render_results_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

    /// Shows the context menu that allows manual labeling/export shortcuts.
    pub(super) fn render_context_menu(&mut self, ui: &mut egui::Ui, indices: &[usize]) {
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
}
