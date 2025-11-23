//! Selection utilities for the results grid.

use super::{UiApp, ViewMode};
use eframe::egui;

impl UiApp {
    /// Returns the indices that should be shown for the requested view mode.
    pub(super) fn indices_for_view(&self, view: ViewMode) -> Vec<usize> {
        self.rijen
            .iter()
            .enumerate()
            .filter_map(|(idx, info)| match view {
                ViewMode::Aanwezig if info.present && !self.is_onzeker(info) => Some(idx),
                ViewMode::Leeg if !info.present && !self.is_onzeker(info) => Some(idx),
                ViewMode::Onzeker if self.is_onzeker(info) => Some(idx),
                _ => None,
            })
            .collect()
    }

    /// Convenience helper that returns the indices for the currently active view tab.
    pub(super) fn filtered_indices(&self) -> Vec<usize> {
        self.indices_for_view(self.view)
    }

    /// Counts how many results fall into each view category.
    pub(super) fn view_counts(&self) -> (usize, usize, usize) {
        let mut present = 0usize;
        let mut empty = 0usize;
        let mut unsure = 0usize;
        for info in &self.rijen {
            if self.is_onzeker(info) {
                unsure += 1;
            } else if info.present {
                present += 1;
            } else {
                empty += 1;
            }
        }
        (present, empty, unsure)
    }

    /// Handles `Cmd+A` to select all tiles in the current grid.
    pub(super) fn handle_select_shortcuts(&mut self, ctx: &egui::Context, filtered: &[usize]) {
        let mut trigger_select_all = false;
        ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::A) {
                trigger_select_all = true;
            }
        });
        if trigger_select_all {
            self.select_all(filtered);
        }
    }

    /// Selects a single index and clears the previous selection.
    pub(super) fn select_single(&mut self, idx: usize) {
        self.selected_indices.clear();
        self.selected_indices.insert(idx);
        self.selection_anchor = Some(idx);
        self.selection_focus = Some(idx);
    }

    /// Toggles the selection state for the provided index.
    pub(super) fn toggle_selection(&mut self, idx: usize) {
        if self.selected_indices.contains(&idx) {
            self.selected_indices.remove(&idx);
        } else {
            self.selected_indices.insert(idx);
            self.selection_anchor = Some(idx);
            self.selection_focus = Some(idx);
        }
    }

    /// Selects a range by extending the current anchor to the clicked tile.
    pub(super) fn select_range_in_view(
        &mut self,
        filtered: &[usize],
        target_idx: usize,
        preserve_anchor: bool,
    ) {
        let anchor_idx = self.selection_anchor.unwrap_or(target_idx);
        let Some(anchor_pos) = filtered.iter().position(|&v| v == anchor_idx) else {
            self.select_single(target_idx);
            return;
        };
        let Some(target_pos) = filtered.iter().position(|&v| v == target_idx) else {
            self.select_single(target_idx);
            return;
        };
        let (start, end) = if anchor_pos <= target_pos {
            (anchor_pos, target_pos)
        } else {
            (target_pos, anchor_pos)
        };
        self.selected_indices.clear();
        for &idx in &filtered[start..=end] {
            self.selected_indices.insert(idx);
        }
        if !preserve_anchor {
            self.selection_anchor = Some(target_idx);
        } else {
            self.selection_anchor = Some(anchor_idx);
        }
        self.selection_focus = Some(target_idx);
    }

    /// Selects every tile that is currently visible.
    pub(super) fn select_all(&mut self, filtered: &[usize]) {
        self.selected_indices.clear();
        for &idx in filtered {
            self.selected_indices.insert(idx);
        }
        self.selection_anchor = filtered.first().copied();
        self.selection_focus = filtered.first().copied();
    }

    /// Applies egui modifier rules to determine the resulting selection after a click.
    pub(super) fn handle_selection_click(
        &mut self,
        filtered: &[usize],
        idx: usize,
        modifiers: egui::Modifiers,
    ) {
        if modifiers.shift {
            self.select_range_in_view(filtered, idx, true);
        } else if modifiers.command {
            self.toggle_selection(idx);
        } else {
            self.select_single(idx);
        }
    }

    /// Clears the current selection and resets the anchor.
    pub(super) fn reset_selection(&mut self) {
        self.selected_indices.clear();
        self.selection_anchor = None;
        self.selection_focus = None;
    }

    /// Returns the currently focused index if it exists within the provided slice.
    pub(super) fn current_focus_index(&self, page_indices: &[usize]) -> Option<usize> {
        if let Some(focus) = self.selection_focus
            && page_indices.contains(&focus)
        {
            return Some(focus);
        }
        if let Some(anchor) = self.selection_anchor
            && page_indices.contains(&anchor)
        {
            return Some(anchor);
        }
        page_indices
            .iter()
            .find(|&&idx| self.selected_indices.contains(&idx))
            .copied()
    }

    fn move_selection_to(&mut self, page_indices: &[usize], target_pos: usize) {
        if page_indices.is_empty() {
            return;
        }
        let clamped = target_pos.min(page_indices.len() - 1);
        self.select_single(page_indices[clamped]);
        self.selection_focus = Some(page_indices[clamped]);
    }

    pub(super) fn move_selection_by(&mut self, page_indices: &[usize], delta: isize, extend: bool) {
        if page_indices.is_empty() {
            return;
        }
        let current = self
            .current_focus_index(page_indices)
            .unwrap_or(page_indices[0]);
        let pos = page_indices.iter().position(|&v| v == current).unwrap_or(0) as isize;
        let len = page_indices.len() as isize;
        let target = (pos + delta).clamp(0, len - 1) as usize;
        let target_idx = page_indices[target];
        if extend {
            self.select_range_in_view(page_indices, target_idx, true);
        } else {
            self.move_selection_to(page_indices, target);
        }
    }

    pub(super) fn move_selection_row_start(
        &mut self,
        page_indices: &[usize],
        columns: usize,
        extend: bool,
    ) {
        if page_indices.is_empty() || columns == 0 {
            return;
        }
        let current = self
            .current_focus_index(page_indices)
            .unwrap_or(page_indices[0]);
        let pos = page_indices.iter().position(|&v| v == current).unwrap_or(0);
        let start = (pos / columns) * columns;
        let target_idx = page_indices[start];
        if extend {
            self.select_range_in_view(page_indices, target_idx, true);
        } else {
            self.move_selection_to(page_indices, start);
        }
    }

    pub(super) fn move_selection_row_end(
        &mut self,
        page_indices: &[usize],
        columns: usize,
        extend: bool,
    ) {
        if page_indices.is_empty() || columns == 0 {
            return;
        }
        let current = self
            .current_focus_index(page_indices)
            .unwrap_or(page_indices[page_indices.len() - 1]);
        let pos = page_indices.iter().position(|&v| v == current).unwrap_or(0);
        let start = (pos / columns) * columns;
        let end = ((start + columns).min(page_indices.len())).saturating_sub(1);
        let target_idx = page_indices[end];
        if extend {
            self.select_range_in_view(page_indices, target_idx, true);
        } else {
            self.move_selection_to(page_indices, end);
        }
    }

    pub(super) fn move_selection_to_start(&mut self, page_indices: &[usize], extend: bool) {
        if page_indices.is_empty() {
            return;
        }
        let target_idx = page_indices[0];
        if extend {
            self.select_range_in_view(page_indices, target_idx, true);
        } else {
            self.move_selection_to(page_indices, 0);
        }
    }

    pub(super) fn move_selection_to_end(&mut self, page_indices: &[usize], extend: bool) {
        if page_indices.is_empty() {
            return;
        }
        let target = page_indices.len() - 1;
        let target_idx = page_indices[target];
        if extend {
            self.select_range_in_view(page_indices, target_idx, true);
        } else {
            self.move_selection_to(page_indices, target);
        }
    }
}
