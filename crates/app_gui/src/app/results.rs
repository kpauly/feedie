//! Rendering of the results grid and associated interactions.

use super::{CARD_HEIGHT, CARD_WIDTH, PAGE_SIZE, THUMB_SIZE, UiApp, ViewMode};
use eframe::egui;
use feeder_core::{Decision, ImageInfo};

const PAGE_SCROLL_STEP: f32 = CARD_HEIGHT + 20.0;

enum PageCommand {
    First,
    Previous,
    Next,
    Last,
}

enum SelectionCommand {
    Move(isize),
    RowStart,
    RowEnd,
    First,
    Last,
}

impl UiApp {
    /// Returns the caption that is shown under every thumbnail.
    pub(super) fn thumbnail_caption(&self, info: &ImageInfo) -> String {
        match &info.classification {
            Some(classification) => {
                let mut label = match &classification.decision {
                    Decision::Label(name) => self.display_for(name),
                    Decision::Unknown => self.tr("Leeg", "Empty").to_string(),
                };
                if matches!(&classification.decision, Decision::Label(name) if name.ends_with(" (manueel)"))
                {
                    label.push_str(self.tr(" (manueel)", " (manual)"));
                }
                format!("{label} ({:.1}%)", classification.confidence * 100.0)
            }
            None => self
                .tr("Geen classificatie", "No classification")
                .to_string(),
        }
    }

    /// Draws a single thumbnail card within the result grid.
    pub(super) fn draw_thumbnail_card(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        is_selected: bool,
    ) -> (egui::Response, egui::Rect) {
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

        let tex_id = self.thumb_texture_id(&file_path);
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
        (response, rect)
    }

    /// Renders the panel that shows scan results and thumbnails.
    pub(super) fn render_results_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.scan_in_progress {
            self.render_progress_ui(ui);
            return;
        }
        if !self.has_scanned {
            ui.label(self.tr("Nog geen scan uitgevoerd.", "No scan has been run yet."));
            return;
        }
        if self.rijen.is_empty() {
            ui.label(self.tr("Geen resultaten beschikbaar.", "No results available."));
            return;
        }

        let (count_present, count_empty, count_unsure) = self.view_counts();
        ui.horizontal(|ui| {
            let present_btn = ui.selectable_label(
                self.view == ViewMode::Aanwezig,
                format!("{} ({count_present})", self.tr("Aanwezig", "Present")),
            );
            let empty_btn = ui.selectable_label(
                self.view == ViewMode::Leeg,
                format!("{} ({count_empty})", self.tr("Leeg", "Empty")),
            );
            let unsure_btn = ui.selectable_label(
                self.view == ViewMode::Onzeker,
                format!("{} ({count_unsure})", self.tr("Onzeker", "Uncertain")),
            );
            if present_btn.clicked() {
                self.view = ViewMode::Aanwezig;
                self.reset_thumbnail_cache();
                self.reset_selection();
                self.current_page = 0;
            }
            if empty_btn.clicked() {
                self.view = ViewMode::Leeg;
                self.reset_thumbnail_cache();
                self.reset_selection();
                self.current_page = 0;
            }
            if unsure_btn.clicked() {
                self.view = ViewMode::Onzeker;
                self.reset_thumbnail_cache();
                self.reset_selection();
                self.current_page = 0;
            }
        });

        let filtered = self.filtered_indices();
        let total_pages = self.total_pages(filtered.len());
        if self.current_page >= total_pages {
            self.current_page = total_pages.saturating_sub(1);
        }
        let (start, end) = self.page_bounds(filtered.len());
        let mut page_indices = &filtered[start..end];
        let columns = self.estimate_columns(ui);
        let (scroll_delta, selection_moved) =
            self.handle_navigation_keys(ctx, page_indices, total_pages, columns);
        let total_pages = self.total_pages(filtered.len());
        if self.current_page >= total_pages {
            self.current_page = total_pages.saturating_sub(1);
        }
        let (start, end) = self.page_bounds(filtered.len());
        page_indices = &filtered[start..end];
        self.handle_select_shortcuts(ctx, page_indices);
        let target_focus = if selection_moved {
            self.current_focus_index(page_indices)
        } else {
            None
        };

        if filtered.is_empty() {
            ui.label(self.tr(
                "Geen frames om te tonen in deze weergave.",
                "No frames to show in this view.",
            ));
        } else {
            self.queue_thumbnails_for_indices(page_indices);
            if self.current_page + 1 < total_pages {
                let next_page = self.current_page + 1;
                let start = next_page * PAGE_SIZE;
                let end = (start + PAGE_SIZE).min(filtered.len());
                self.queue_thumbnails_for_indices(&filtered[start..end]);
            }
            let mut loaded_on_page = 0usize;
            for &idx in page_indices {
                if let Some(info) = self.rijen.get(idx)
                    && self.thumbs.contains_key(&info.file)
                {
                    loaded_on_page += 1;
                }
            }
            ui.add_space(6.0);
            self.render_page_controls(ui, total_pages);
            if loaded_on_page < page_indices.len() {
                ui.label(format!(
                    "{}: {loaded_on_page} / {}",
                    self.tr("Thumbnails laden", "Loading thumbnails"),
                    page_indices.len()
                ));
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if scroll_delta.abs() > f32::EPSILON {
                        ui.scroll_with_delta(egui::vec2(0.0, scroll_delta));
                    }
                    let mut target_rect: Option<egui::Rect> = None;
                    ui.horizontal_wrapped(|ui| {
                        for &idx in page_indices {
                            let is_selected = self.selected_indices.contains(&idx);
                            let (response, rect) = self.draw_thumbnail_card(ui, idx, is_selected);
                            if Some(idx) == target_focus {
                                target_rect = Some(rect);
                            }
                            if response.clicked() {
                                let modifiers = ctx.input(|i| i.modifiers);
                                self.handle_selection_click(page_indices, idx, modifiers);
                            }
                            if response.double_clicked() {
                                self.open_preview(&filtered, idx);
                            }
                        }
                    });
                    if selection_moved {
                        if let Some(rect) = target_rect {
                            ui.scroll_to_rect(rect, Some(egui::Align::Center));
                        }
                    } else if let Some(rect) = target_rect {
                        ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    }
                });
            ui.add_space(4.0);
            self.render_page_controls(ui, total_pages);
        }
    }

    /// Shows the context menu that allows manual labeling/export shortcuts.
    pub(super) fn render_context_menu(&mut self, ui: &mut egui::Ui, indices: &[usize]) {
        if ui.button(self.tr("Exporteren", "Export")).clicked() {
            self.export_selected_images(indices);
            ui.close();
        }
        ui.separator();
        if ui
            .button(self.tr(
                "Markeer als Achtergrond (Leeg)",
                "Mark as Background (Empty)",
            ))
            .clicked()
        {
            self.assign_manual_category(indices, "achtergrond".into(), false);
            ui.close();
        }
        if ui
            .button(self.tr(
                "Markeer als Iets sp. (Onzeker)",
                "Mark as Something sp. (Uncertain)",
            ))
            .clicked()
        {
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
        let new_menu_label = self.tr("Nieuw...", "New...");
        ui.menu_button(new_menu_label, |ui| {
            let new_label_prompt =
                self.tr("Vul een nieuwe soortnaam in:", "Enter a new species name:");
            ui.label(new_label_prompt);
            ui.horizontal(|ui| {
                let new_label_hint = self.tr("Nieuwe soort", "New species");
                let ok_label = self.tr("OK", "OK");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.new_label_buffer)
                        .hint_text(new_label_hint),
                );
                resp.request_focus();
                let mut submit = false;
                if resp.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !self.new_label_buffer.trim().is_empty()
                {
                    submit = true;
                }
                if ui.button(ok_label).clicked() {
                    submit = true;
                }
                if submit && self.apply_new_label(indices) {
                    ui.close();
                }
            });
        });
    }
}

impl UiApp {
    fn total_pages(&self, len: usize) -> usize {
        if len == 0 { 1 } else { len.div_ceil(PAGE_SIZE) }
    }

    fn page_bounds(&self, len: usize) -> (usize, usize) {
        let start = self.current_page.saturating_mul(PAGE_SIZE);
        let end = (start + PAGE_SIZE).min(len);
        (start, end)
    }

    fn render_page_controls(&mut self, ui: &mut egui::Ui, total_pages: usize) {
        ui.horizontal(|ui| {
            let current = self.current_page;
            let label = format!(
                "{} {} | {}",
                self.tr("Pagina", "Page"),
                current + 1,
                total_pages
            );
            if ui
                .add_enabled(current > 0, egui::Button::new("<<"))
                .clicked()
            {
                self.goto_page(0, total_pages);
            }
            if ui
                .add_enabled(current > 0, egui::Button::new("<"))
                .clicked()
            {
                self.change_page_relative(-1, total_pages);
            }
            ui.label(label);
            if ui
                .add_enabled(current + 1 < total_pages, egui::Button::new(">"))
                .clicked()
            {
                self.change_page_relative(1, total_pages);
            }
            if ui
                .add_enabled(current + 1 < total_pages, egui::Button::new(">>"))
                .clicked()
            {
                self.goto_page(total_pages.saturating_sub(1), total_pages);
            }
        });
    }

    fn goto_page(&mut self, new_page: usize, total_pages: usize) {
        if total_pages == 0 {
            return;
        }
        let target = new_page.min(total_pages.saturating_sub(1));
        if self.current_page != target {
            self.current_page = target;
            self.reset_selection();
        }
    }

    fn change_page_relative(&mut self, delta: isize, total_pages: usize) {
        let current = self.current_page as isize;
        let target = current + delta;
        if target < 0 {
            self.goto_page(0, total_pages);
        } else {
            self.goto_page(target as usize, total_pages);
        }
    }

    fn estimate_columns(&self, ui: &egui::Ui) -> usize {
        let spacing = ui.spacing().item_spacing.x;
        let width = (CARD_WIDTH + spacing).max(1.0);
        let available = ui.available_width().max(width);
        ((available + spacing) / width).floor().max(1.0) as usize
    }

    fn handle_navigation_keys(
        &mut self,
        ctx: &egui::Context,
        page_indices: &[usize],
        total_pages: usize,
        columns: usize,
    ) -> (f32, bool) {
        let has_focus = self.current_focus_index(page_indices).is_some();
        let mut scroll_delta = 0.0f32;
        let mut page_cmd: Option<PageCommand> = None;
        let mut selection_cmd: Option<SelectionCommand> = None;
        let mut extend_selection = false;
        let mut selection_moved = false;
        ctx.input_mut(|input| {
            extend_selection = input.modifiers.shift;
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::PageDown) {
                page_cmd = Some(PageCommand::Last);
                return;
            }
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::PageUp) {
                page_cmd = Some(PageCommand::First);
                return;
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::PageDown) {
                page_cmd = Some(PageCommand::Next);
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::PageUp) {
                page_cmd = Some(PageCommand::Previous);
            }

            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Home) {
                selection_cmd = Some(SelectionCommand::First);
                return;
            }
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::End) {
                selection_cmd = Some(SelectionCommand::Last);
                return;
            }

            if has_focus {
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                    selection_cmd = Some(SelectionCommand::Move(-1));
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                    selection_cmd = Some(SelectionCommand::Move(1));
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    let step = columns.max(1) as isize;
                    selection_cmd = Some(SelectionCommand::Move(-step));
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    let step = columns.max(1) as isize;
                    selection_cmd = Some(SelectionCommand::Move(step));
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                    selection_cmd = Some(SelectionCommand::RowStart);
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::End) {
                    selection_cmd = Some(SelectionCommand::RowEnd);
                }
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                scroll_delta -= PAGE_SCROLL_STEP;
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                scroll_delta += PAGE_SCROLL_STEP;
            }
        });

        match page_cmd {
            Some(PageCommand::First) => self.goto_page(0, total_pages),
            Some(PageCommand::Last) => self.goto_page(total_pages.saturating_sub(1), total_pages),
            Some(PageCommand::Next) => self.change_page_relative(1, total_pages),
            Some(PageCommand::Previous) => self.change_page_relative(-1, total_pages),
            None => {}
        }

        match selection_cmd {
            Some(SelectionCommand::Move(delta)) => {
                self.move_selection_by(page_indices, delta, extend_selection);
                selection_moved = true;
            }
            Some(SelectionCommand::RowStart) => {
                self.move_selection_row_start(page_indices, columns.max(1), extend_selection);
                selection_moved = true;
            }
            Some(SelectionCommand::RowEnd) => {
                self.move_selection_row_end(page_indices, columns.max(1), extend_selection);
                selection_moved = true;
            }
            Some(SelectionCommand::First) => {
                self.move_selection_to_start(page_indices, extend_selection);
                selection_moved = true;
            }
            Some(SelectionCommand::Last) => {
                self.move_selection_to_end(page_indices, extend_selection);
                selection_moved = true;
            }
            None => {}
        }

        (scroll_delta, selection_moved)
    }
}
