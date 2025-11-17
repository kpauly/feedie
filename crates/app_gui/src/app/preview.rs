//! Management of the floating image preview window.

use super::{UiApp, ViewMode};
use eframe::egui;
use feeder_core::Decision;

/// Actions a preview session can request from the controller.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PreviewAction {
    None,
    Prev,
    Next,
    Close,
}

/// State that powers the separate preview viewport.
#[derive(Clone)]
pub(crate) struct PreviewState {
    pub(super) view: ViewMode,
    pub(super) current: usize,
    pub(super) open: bool,
    pub(super) viewport_id: egui::ViewportId,
    pub(super) initialized: bool,
}

impl UiApp {
    /// Initializes the preview viewer for the provided result index.
    pub(super) fn open_preview(&mut self, filtered: &[usize], idx: usize) {
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

    /// Renders the floating preview window when requested.
    pub(super) fn render_preview_window(&mut self, ctx: &egui::Context) {
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
                        .add_enabled(!prev_disabled, egui::Button::new("< Vorige"))
                        .clicked()
                    {
                        action = PreviewAction::Prev;
                    }
                    if wants_prev && !prev_disabled {
                        action = PreviewAction::Prev;
                    }
                    let next_disabled = preview.current + 1 >= indices.len();
                    if ui
                        .add_enabled(!next_disabled, egui::Button::new("Volgende >"))
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
}
